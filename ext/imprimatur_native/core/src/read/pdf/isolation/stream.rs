//! Flushed page records survive a worker timeout or resource termination.

use std::io::Write;

use serde::{Deserialize, Serialize};

use super::{Output, protocol};
use crate::read::pdf::{Info, PageResult, Pages, ReadOptions, Status, bounded};
use crate::{Error, Result};

#[derive(Serialize, Deserialize)]
pub(super) enum Frame {
    Info(Info),
    Page(PageResult),
    End,
    Error { kind: u8, message: String },
}

pub(super) fn write(out: &mut impl Write, frame: &Frame) -> Result<()> {
    serde_json::to_writer(&mut *out, frame)
        .map_err(|error| Error::malformed("PDF worker", error.to_string()))?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

pub(super) fn decode(output: Output, options: &ReadOptions) -> Result<Pages> {
    decode_recoverable(output, options).map(|(pages, _)| pages)
}

// The index comes from the supervisor/protocol failure, not human-readable
// page reasons: output-limit skips must never become recovery candidates.
pub(super) fn decode_recoverable(
    output: Output,
    options: &ReadOptions,
) -> Result<(Pages, Option<usize>)> {
    let mut failure = output.failure;
    if output.data.len() < protocol::HEADER.len() {
        return Err(failure.unwrap_or_else(|| malformed("missing response")));
    }
    let bytes =
        protocol::decode(output.data, protocol::Operation::Pages(options.clone()).maximum()?)?;
    let mut info = None;
    let mut pages = Vec::new();
    let mut text_bytes = 0_usize;
    let mut ended = false;
    for line in bytes.split_inclusive(|&byte| byte == b'\n') {
        if ended {
            return Err(malformed("records after the end of the response"));
        }
        if !line.ends_with(b"\n") && failure.is_some() {
            break; // A killed worker may leave one incomplete final record.
        }
        let frame: Frame =
            serde_json::from_slice(line).map_err(|error| malformed(&error.to_string()))?;
        match frame {
            Frame::Info(value) if info.is_none() && pages.is_empty() => info = Some(value),
            Frame::Page(page) => {
                let metadata = info.ok_or_else(|| malformed("page before metadata"))?;
                validate(&page, pages.len(), metadata, options)?;
                text_bytes = text_bytes
                    .checked_add(page.text.len())
                    .filter(|&bytes| bytes <= options.max_output_bytes)
                    .ok_or_else(|| malformed("text exceeds the requested limit"))?;
                pages.push(page);
            }
            Frame::End => ended = true,
            Frame::Error { kind, message } => {
                failure = Some(protocol::error(kind, message));
                ended = true;
            }
            Frame::Info(_) => return Err(malformed("duplicate metadata")),
        }
    }
    let info =
        info.ok_or_else(|| failure.take().unwrap_or_else(|| malformed("missing metadata")))?;
    let count = selected(info, options);
    if pages.len() == count && ended {
        return Ok((Pages { info, pages }, None));
    }
    let error = failure.ok_or_else(|| malformed("incomplete response"))?;
    let reason = error.to_string();
    let recoverable = expired(&error);
    let timed_out = recoverable || matches!(error, Error::LimitExceeded("PDF worker limit"));
    let first_missing = pages.len();
    for index in first_missing..count {
        let number =
            options.first_page + u32::try_from(index).map_err(|_| malformed("page index"))?;
        let status =
            if timed_out || index > first_missing { Status::Skipped } else { Status::Failed };
        pages.push(bounded::unread(number, status, reason.clone()));
    }
    Ok((Pages { info, pages }, recoverable.then_some(first_missing)))
}

pub(super) fn expired(error: &Error) -> bool {
    matches!(error, Error::LimitExceeded("time" | "render worker wall time"))
}

fn selected(info: Info, options: &ReadOptions) -> usize {
    let count = options
        .last_page
        .min(info.page_count)
        .checked_sub(options.first_page)
        .map_or(0, |difference| u64::from(difference) + 1);
    usize::try_from(count).unwrap_or(usize::MAX).min(options.max_pages)
}

fn validate(page: &PageResult, index: usize, info: Info, options: &ReadOptions) -> Result<()> {
    let expected = u64::from(options.first_page) + index as u64;
    if index >= selected(info, options)
        || u64::from(page.number) != expected
        || page.text.len() > options.max_page_bytes
        || page.reason.as_ref().is_some_and(|reason| reason.len() > protocol::MAX_ERROR_BYTES)
        || (matches!(page.status, Status::Failed | Status::Skipped) && !page.text.is_empty())
    {
        return Err(malformed("invalid page record"));
    }
    Ok(())
}

fn malformed(reason: &str) -> Error {
    Error::malformed("PDF worker", reason)
}
