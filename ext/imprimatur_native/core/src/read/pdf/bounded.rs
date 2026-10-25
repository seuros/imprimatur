//! PDF inspection and bounded reads without losing individual page failures.

#[cfg(test)]
mod tests;

use super::content::{Fonts, PageText, page_text};
use super::document::Document;
use super::{is_pdf, layout, xref};
use crate::read::{Budget, Limits};
use crate::{Error, Result};

/// Metadata read without interpreting page content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "isolation", derive(serde::Serialize, serde::Deserialize))]
pub struct Info {
    pub page_count: u32,
    /// Opened with the empty password: permissions, not a locked document.
    pub encrypted: bool,
}

/// A contiguous, one-based page selection and its output limits.
#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub first_page: u32,
    pub last_page: u32,
    pub max_pages: usize,
    pub max_page_bytes: usize,
    pub max_output_bytes: usize,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            first_page: 1,
            last_page: u32::MAX,
            max_pages: 500,
            max_page_bytes: 256 * 1024,
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ReadOptions {
    pub(super) const fn validate(&self) -> Result<()> {
        if self.first_page == 0 || self.last_page < self.first_page {
            return Err(Error::InvalidPdfOption("pages must be an ascending one-based range"));
        }
        if self.max_page_bytes == 0 || self.max_output_bytes == 0 {
            return Err(Error::InvalidPdfOption("text limits must be positive"));
        }
        Ok(())
    }
}

/// What is known about a page; failure is never reported as empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "isolation", derive(serde::Serialize, serde::Deserialize))]
pub enum Status {
    Text,
    Empty,
    Failed,
    Skipped,
}

impl Status {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Empty => "empty",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "isolation", derive(serde::Serialize, serde::Deserialize))]
pub struct PageResult {
    pub number: u32,
    pub status: Status,
    pub text: String,
    /// Unknown if the page could not be interpreted.
    pub images: Option<u32>,
    pub truncated: bool,
    pub reason: Option<String>,
}

#[derive(Debug)]
pub struct Pages {
    pub info: Info,
    pub pages: Vec<PageResult>,
}

/// Counts actual leaves, not the page tree's advertised `/Count`.
///
/// # Errors
/// A malformed, locked, unsupported or over-budget document.
pub fn info(data: &[u8], limits: &Limits) -> Result<Info> {
    let budget = Budget::new(limits, data.len())?;
    let document = open(data, &budget)?;
    let selected = document.select_pages(1, u32::MAX, 0)?;
    Ok(Info { page_count: selected.count, encrypted: document.encrypted() })
}

/// Reads only selected page streams. Page failures are retained alongside
/// successes; cancellation and exhausted document budgets remain errors.
///
/// # Errors
/// As [`info`], and invalid options.
pub fn pages(data: &[u8], options: &ReadOptions, limits: &Limits) -> Result<Pages> {
    let mut pages = Vec::new();
    let info = visit_pages(data, options, limits, false, |page| {
        pages.push(page);
        Ok(())
    })?;
    Ok(Pages { info, pages })
}

/// The worker flushes each completed page. After an exhausted stream budget,
/// single-page attempts get fresh decompression budgets, as individual reads
/// would; the supervisor still enforces one wall-clock and process limit.
pub(super) fn visit_pages(
    data: &[u8],
    options: &ReadOptions,
    limits: &Limits,
    recover: bool,
    mut emit: impl FnMut(PageResult) -> Result<()>,
) -> Result<Info> {
    options.validate()?;
    let budget = Budget::new(limits, data.len())?;
    let document = open(data, &budget)?;
    let selected =
        document.select_pages(options.first_page, options.last_page, options.max_pages)?;
    let mut fonts = Fonts::new();
    let mut text = PageText { strict: true, ..PageText::default() };
    let mut remaining = options.max_output_bytes;
    let mut individual = false;
    for (number, page) in selected.pages {
        budget.check()?;
        if remaining == 0 {
            emit(unread(number, Status::Skipped, "the text output limit was reached".into()))?;
            continue;
        }
        text.max_bytes = Some(options.max_page_bytes.min(remaining));
        let read = if individual {
            single_page(data, number, options, remaining, limits)
        } else {
            page.and_then(|page| page_text(&document, &page, &mut fonts, &mut text))
                .map(|()| text_result(number, &text, text.max_bytes.unwrap_or(remaining)))
        };
        let read = match read {
            Err(error @ Error::LimitExceeded(_)) if recover => {
                // Never renew an expired time budget or swallow cancellation.
                budget.check()?;
                if individual {
                    Err(error)
                } else {
                    individual = true;
                    single_page(data, number, options, remaining, limits)
                }
            }
            other => other,
        };
        let result = match read {
            Ok(page) => page,
            Err(error @ Error::LimitExceeded(_)) if recover => {
                budget.check()?;
                unread(number, Status::Failed, error.to_string())
            }
            Err(error) if xref::is_limit(&error) || matches!(error, Error::PasswordRequired) => {
                return Err(error);
            }
            Err(error) => unread(number, Status::Failed, error.to_string()),
        };
        remaining -= result.text.len();
        emit(result)?;
    }
    Ok(Info { page_count: selected.count, encrypted: document.encrypted() })
}

fn single_page(
    data: &[u8],
    number: u32,
    options: &ReadOptions,
    remaining: usize,
    limits: &Limits,
) -> Result<PageResult> {
    let options = ReadOptions {
        first_page: number,
        last_page: number,
        max_pages: 1,
        max_output_bytes: remaining,
        ..options.clone()
    };
    pages(data, &options, limits)?
        .pages
        .pop()
        .ok_or_else(|| Error::malformed("PDF", "the selected page disappeared"))
}

fn text_result(number: u32, text: &PageText, limit: usize) -> PageResult {
    let lines = layout::lines(text);
    let mut output = String::new();
    for line in &lines.lines {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(lines.text(line));
    }
    let truncated = text.truncated || output.len() > limit;
    truncate(&mut output, limit);
    PageResult {
        number,
        status: if output.trim().is_empty() { Status::Empty } else { Status::Text },
        text: output,
        images: Some(u32::try_from(text.images).unwrap_or(u32::MAX)),
        truncated,
        reason: truncated.then(|| "the page text limit was reached".into()),
    }
}

pub(super) fn open<'a>(data: &'a [u8], budget: &'a Budget<'a>) -> Result<Document<'a>> {
    if !is_pdf(data) {
        return Err(Error::UnsupportedDocument("not a PDF file".into()));
    }
    budget.check()?;
    Document::open(data, budget).map(Document::strict)
}

pub(super) fn unread(number: u32, status: Status, mut reason: String) -> PageResult {
    truncate(&mut reason, 4096);
    PageResult {
        number,
        status,
        reason: Some(reason),
        text: String::new(),
        images: None,
        truncated: false,
    }
}

fn truncate(text: &mut String, maximum: usize) {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}
