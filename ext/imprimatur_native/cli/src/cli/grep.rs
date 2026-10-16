use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use imprimatur::read::{Block, Content};
use regex::{Regex, RegexBuilder};
use serde_json::json;
use usage_rs::{Args, RunWith};

use super::limits::LimitFlags;
use crate::app::{self, App, Out};

/// Search documents for a pattern
///
/// Searches what `imprimatur markdown` reads, line by line: a PDF's
/// paragraphs and headings by page, a workbook's cells and comments by
/// sheet and cell, a Word document's blocks by section. Each match is
/// printed with where it is: `p3` (page 3), `Sales!B2` (a cell, or
/// `Sales!B2 (comment)` its comment), `§Scope` (the heading it falls under)
/// or `¶12` (the 12th block, before any heading).
///
/// The exit status follows grep: 0 when a line matched, 1 when none did,
/// 2 for a wrong command line or a document that could not be searched (a
/// scanned PDF, a damaged file); with `--json`, its reason is reported.
#[derive(Args, Debug)]
#[expect(clippy::struct_excessive_bools, reason = "grep's switches, each a flag of its own")]
#[usage(
    effect = "read",
    args_override_self = false,
    example("imprimatur grep -i 'non.?conformity' audits/*.pdf", help = "Regular expression"),
    example("imprimatur grep -F 'ISO 17065' -C 1 cert.pdf", help = "A fixed string, with context"),
    example("imprimatur grep -l GOTS *.docx *.xlsx", help = "Only the documents that match"),
    example("imprimatur --json grep 'Total' sales.xlsx", help = "Matches as JSON, with offsets")
)]
pub struct Grep {
    /// A regular expression (Rust syntax), or with `-F` a fixed string
    #[usage(arg, value_name = "PATTERN")]
    pub pattern: String,

    /// Documents to search; `-` reads standard input
    #[usage(arg, required, value_name = "FILE", value_hint = usage_rs::ValueHint::FilePath)]
    pub files: Vec<PathBuf>,

    /// Match regardless of case
    #[usage(short = 'i', long)]
    pub ignore_case: bool,

    /// Take the pattern as a fixed string
    #[usage(short = 'F', long)]
    pub fixed_strings: bool,

    /// Match whole words only
    #[usage(short = 'w', long)]
    pub word_regexp: bool,

    /// Lines of context around each match
    #[usage(short = 'C', long, default = "0", value_name = "LINES")]
    pub context: usize,

    /// Stop each document after this many matching lines
    #[usage(short = 'm', long, value_name = "NUMBER")]
    pub max_count: Option<usize>,

    /// Print only the documents with a match
    #[usage(short = 'l', long, conflicts("--count"))]
    pub files_with_matches: bool,

    /// Print how many lines match in each document
    #[usage(short = 'c', long)]
    pub count: bool,

    /// Print nothing; the exit status tells whether anything matched
    #[usage(short = 'q', long)]
    pub quiet: bool,

    #[usage(flatten)]
    pub limits: LimitFlags,
}

/// Where a line is.
#[derive(Debug, Clone)]
enum Location<'c> {
    Page(u32),
    Cell { sheet: &'c str, cell: String },
    Comment { sheet: &'c str, cell: String },
    Section(&'c str),
    Block(usize),
}

impl Location<'_> {
    fn label(&self, out: &mut String) {
        // Writing to a String cannot fail.
        let _ = match self {
            Self::Page(page) => write!(out, "p{page}"),
            Self::Cell { sheet, cell } => write!(out, "{sheet}!{cell}"),
            Self::Comment { sheet, cell } => write!(out, "{sheet}!{cell} (comment)"),
            Self::Section(heading) => write!(out, "§{heading}"),
            Self::Block(index) => write!(out, "¶{index}"),
        };
    }

    fn json(&self) -> serde_json::Value {
        match self {
            Self::Page(page) => json!({ "page": page }),
            Self::Cell { sheet, cell } => json!({ "sheet": sheet, "cell": cell }),
            Self::Comment { sheet, cell } => {
                json!({ "sheet": sheet, "cell": cell, "comment": true })
            }
            Self::Section(heading) => json!({ "section": heading }),
            Self::Block(index) => json!({ "block": index }),
        }
    }
}

/// Every line of a document's content, with where it is.
fn lines<'c>(content: &'c Content, mut each: impl FnMut(Location<'c>, &str) -> bool) {
    match content {
        Content::Pdf(pages) => {
            for page in pages {
                for placed in &page.blocks {
                    for line in placed.block.plain_text().lines() {
                        if !each(Location::Page(page.number), line) {
                            return;
                        }
                    }
                }
            }
        }
        Content::Xlsx(sheets) => {
            for sheet in sheets {
                for cell in sheet.cells.cells() {
                    let reference = cell.reference();
                    for line in sheet.cells.text(cell).lines() {
                        let location =
                            Location::Cell { sheet: &sheet.name, cell: reference.clone() };
                        if !each(location, line) {
                            return;
                        }
                    }
                }
                for note in &sheet.comments {
                    for line in note.text.lines() {
                        let location =
                            Location::Comment { sheet: &sheet.name, cell: note.reference() };
                        if !each(location, line) {
                            return;
                        }
                    }
                }
            }
        }
        Content::Docx(blocks) => {
            let mut section = None;
            for (index, block) in (1..).zip(blocks) {
                if let Block::Heading { text, .. } = block {
                    section = Some(text.text.as_str());
                }
                let location = section.map_or(Location::Block(index), Location::Section);
                for line in block.plain_text().lines() {
                    if !each(location.clone(), line) {
                        return;
                    }
                }
            }
        }
        _ => {}
    }
}

/// grep's statuses.
const MATCHED: u8 = 0;
const NO_MATCH: u8 = 1;
const TROUBLE: u8 = 2;

impl RunWith<&App> for Grep {
    type Output = ExitCode;

    fn run_with(self, app: &App) -> ExitCode {
        let limits = match self.limits.checked() {
            Ok(limits) => limits,
            Err(status) => return status,
        };
        let pattern = match self.regex() {
            Ok(pattern) => pattern,
            Err(message) => {
                eprintln!("imprimatur: {message}");
                return ExitCode::from(TROUBLE);
            }
        };
        let (mut matched, mut trouble) = (false, false);
        let mut out = Out::new();
        let named = self.files.len() > 1;
        for file in &self.files {
            let document = match app::content(file, &limits) {
                Ok(document) => document,
                Err(error) => {
                    trouble = true;
                    if !self.quiet && !app.report_failure(&mut out, file, &error).1 {
                        break;
                    }
                    continue;
                }
            };
            let search = Search { grep: &self, app: *app, file, named, pattern: &pattern };
            let found = search.run(&document, &mut out);
            matched |= found.count > 0;
            if !found.open || (self.quiet && matched) {
                break;
            }
        }
        ExitCode::from(match (matched, trouble) {
            (true, _) if self.quiet => MATCHED,
            (_, true) => TROUBLE,
            (true, false) => MATCHED,
            (false, false) => NO_MATCH,
        })
    }
}

impl Grep {
    fn regex(&self) -> Result<Regex, String> {
        let escaped;
        let pattern = if self.fixed_strings {
            escaped = regex::escape(&self.pattern);
            &escaped
        } else {
            &self.pattern
        };
        let pattern =
            if self.word_regexp { format!(r"\b(?:{pattern})\b") } else { pattern.clone() };
        RegexBuilder::new(&pattern)
            .case_insensitive(self.ignore_case)
            .build()
            .map_err(|error| format!("invalid pattern: {error}"))
    }
}

struct Search<'g> {
    grep: &'g Grep,
    app: App,
    file: &'g Path,
    named: bool,
    pattern: &'g Regex,
}

/// What searching one document found.
struct Found {
    count: usize,
    open: bool,
}

impl Search<'_> {
    fn run(&self, content: &Content, out: &mut Out) -> Found {
        let grep = self.grep;
        let listing = grep.quiet || grep.files_with_matches || grep.count;
        let limit = grep.max_count.unwrap_or(usize::MAX);
        let mut found = Found { count: 0, open: true };
        // Lines before the next match, and how many after the last to show.
        let mut before: VecDeque<(Location<'_>, String)> = VecDeque::with_capacity(grep.context);
        let (mut after, mut last_shown) = (0_usize, None::<usize>);
        let mut line_number = 0_usize;
        let mut line = String::new();
        lines(content, |location, text| {
            line_number += 1;
            if found.count >= limit && after == 0 {
                return false;
            }
            let hit = found.count < limit && self.pattern.is_match(text);
            if hit {
                found.count += 1;
                if listing {
                    return !(grep.quiet || grep.files_with_matches);
                }
                if grep.context > 0
                    && let Some(shown) = last_shown
                    && line_number - before.len() > shown + 1
                    && !self.app.json
                {
                    found.open &= out.write("--\n");
                }
                let first = line_number - before.len();
                for (offset, (location, text)) in before.iter().enumerate() {
                    found.open &= self.print(out, &mut line, location, text, None, first + offset);
                }
                before.clear();
                found.open &=
                    self.print(out, &mut line, &location, text, Some(self.pattern), line_number);
                last_shown = Some(line_number);
                after = grep.context;
            } else if after > 0 && !listing {
                after -= 1;
                found.open &= self.print(out, &mut line, &location, text, None, line_number);
                last_shown = Some(line_number);
            } else if grep.context > 0 && !listing {
                if before.len() == grep.context {
                    before.pop_front();
                }
                before.push_back((location, text.to_owned()));
            }
            found.open
        });
        if grep.files_with_matches && found.count > 0 {
            found.open &= if self.app.json {
                out.json(&json!({ "file": self.file.to_string_lossy(), "matched": true }))
            } else {
                out.write(&format!("{}\n", self.file.display()))
            };
        } else if grep.count && !grep.quiet {
            found.open &= if self.app.json {
                out.json(&json!({ "file": self.file.to_string_lossy(), "count": found.count }))
            } else if self.named {
                out.write(&format!("{}:{}\n", self.file.display(), found.count))
            } else {
                out.write(&format!("{}\n", found.count))
            };
        }
        found
    }

    /// One line of output: a match (`pattern` given) or context.
    fn print(
        &self,
        out: &mut Out,
        buffer: &mut String,
        location: &Location<'_>,
        text: &str,
        pattern: Option<&Regex>,
        line: usize,
    ) -> bool {
        if self.app.json {
            let mut object = json!({
                "file": self.file.to_string_lossy(),
                "type": if pattern.is_some() { "match" } else { "context" },
                "location": location.json(),
                "line": line,
                "text": text,
            });
            if let Some(pattern) = pattern {
                let spans: Vec<_> = pattern
                    .find_iter(text)
                    .map(|found| json!({ "start": found.start(), "end": found.end() }))
                    .collect();
                object["matches"] = spans.into();
            }
            return out.json(&object);
        }
        buffer.clear();
        if self.named {
            buffer.push_str(&self.file.to_string_lossy());
            buffer.push(':');
        }
        location.label(buffer);
        buffer.push_str(if pattern.is_some() { ": " } else { "- " });
        buffer.push_str(text.trim());
        buffer.push('\n');
        out.write(buffer)
    }
}
