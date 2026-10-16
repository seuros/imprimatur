use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use imprimatur::read::json;
use serde_json::Value as Json;
use usage_rs::{Args, RunWith};

use super::limits::LimitFlags;
use crate::app::{self, App, Out, Status};

/// Print documents as Markdown
///
/// PDF: the text layer, placed glyph by glyph, with headings by font size,
/// running headers and footers left out, and pages after the first marked
/// `<!-- page N -->`. Encrypted PDFs open when their user password is
/// empty. XLSX: each sheet as a table, values as Excel shows them, then its
/// comments. DOCX: headings, lists, emphasis, links and tables.
///
/// With several documents, each is introduced by `<!-- file: PATH -->`. A
/// document that fails is reported on standard error and the others are
/// still read; the exit status is the first failure's.
///
/// With `--json`, each document is one JSON object instead: its structure,
/// not Markdown. A PDF's pages of blocks (heading, paragraph) with their
/// position on the page; a workbook's sheets of rows of typed cells
/// (number, date, bool, text, error, formula) and comments; a Word
/// document's blocks (heading, paragraph, list item, table) with the byte
/// ranges of their bold, italic and linked text.
#[derive(Args, Debug)]
#[usage(
    effect = "read",
    // Per command: the root's setting covers only the root's own flags.
    args_override_self = false,
    after_long_help = super::EXIT_STATUS,
    example("imprimatur markdown audit.pdf", help = "Print a PDF's text"),
    example("imprimatur md -o notes/ *.docx", help = "Write notes/NAME.md for each"),
    example("curl -s $URL | imprimatur md -", help = "Read standard input"),
    example("imprimatur --json md sales.xlsx", help = "Typed cells, as JSON")
)]
pub struct Markdown {
    /// Documents to read; `-` reads standard input
    #[usage(arg, required, value_name = "FILE", value_hint = usage_rs::ValueHint::FilePath)]
    pub files: Vec<PathBuf>,

    /// Write each document to DIR/NAME.md instead of standard output
    #[usage(short = 'o', long, value_name = "DIR", value_hint = usage_rs::ValueHint::DirPath)]
    pub output: Option<PathBuf>,

    #[usage(flatten)]
    pub limits: LimitFlags,
}

impl RunWith<&App> for Markdown {
    type Output = ExitCode;

    fn run_with(self, app: &App) -> ExitCode {
        let limits = match self.limits.checked() {
            Ok(limits) => limits,
            Err(status) => return status,
        };
        if let Some(directory) = &self.output
            && let Err(status) = app::create_dir(directory)
        {
            return status;
        }
        let mut status = Status::default();
        let mut written = HashSet::new();
        let mut out = Out::new();
        let introduce = self.files.len() > 1 && self.output.is_none() && !app.json;
        for file in &self.files {
            let started = Instant::now();
            let read = app::content(file, &limits);
            let milliseconds = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let shown = read.and_then(|content| {
                let target = match &self.output {
                    Some(directory) => {
                        let target =
                            app::unique_path(directory, &app::stem(file), "md", &mut written);
                        fs::write(&target, content.to_markdown())?;
                        Some(target)
                    }
                    None => None,
                };
                Ok((content, target))
            });
            let open = match shown {
                Ok((content, target)) if app.json => {
                    let mut object = json::content(&content);
                    object.insert("file".into(), file.to_string_lossy().into());
                    object.insert("ok".into(), true.into());
                    object.insert("milliseconds".into(), milliseconds.into());
                    if let Some(target) = target {
                        object.insert("written".into(), target.to_string_lossy().into());
                    }
                    out.json(&Json::Object(object))
                }
                Ok((_, Some(_))) => true,
                Ok((content, None)) => out.write(&printed(file, &content.to_markdown(), introduce)),
                Err(error) => {
                    let (failure, open) = app.report_failure(&mut out, file, &error);
                    status.record(failure);
                    open
                }
            };
            if !open {
                break;
            }
        }
        status.exit_code()
    }
}

fn printed(file: &Path, text: &str, introduce: bool) -> String {
    let mut shown = String::with_capacity(text.len() + 64);
    if introduce {
        shown.push_str("<!-- file: ");
        shown.push_str(&file.to_string_lossy());
        shown.push_str(" -->\n\n");
    }
    shown.push_str(text);
    if !text.ends_with('\n') {
        shown.push('\n');
    }
    shown
}
