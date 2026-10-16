//! The command line: the root, its global flag, and the commands.

mod completion;
mod grep;
mod images;
mod limits;
mod markdown;
#[cfg(test)]
mod tests;

use usage_rs::{Cli, Subcommands};

pub use completion::Completion;
pub use grep::Grep;
pub use images::Images;
pub use markdown::Markdown;

/// The exit statuses, for `--help`; `exit_code` below declares them for the
/// spec, and `app::Failure` returns them.
pub const EXIT_STATUS: &str = "Exit status:
0: every document was read
1: a document is damaged, or a file could not be read or written
2: the command line is wrong
3: a PDF has no text layer: its pages are scans, which need OCR
4: a PDF is encrypted with a password
5: not a PDF, XLSX or DOCX (legacy .xls and .doc, .xlsb, presentations)
6: a limit was hit: size, decompressed size, nesting or time";

/// Read PDF, XLSX and DOCX documents: as Markdown or JSON, searched, or their images
///
/// The format is told by a document's content, not its name. Documents are
/// untrusted input: reading is bounded in size, decompressed size, nesting
/// and time, so a hostile file fails fast instead of hanging.
///
/// `grep` exits as grep does: 0 when a line matched, 1 when none did, 2 on
/// trouble. The other commands exit as listed below.
#[derive(Cli, Debug)]
#[usage(
    bin = "imprimatur",
    version,
    completion,
    unknown_flags = "error",
    args_override_self = false,
    exit_code(0, "every document was read"),
    exit_code(1, "a document is damaged, or a file could not be read or written"),
    exit_code(2, "the command line is wrong"),
    exit_code(3, "a PDF has no text layer: its pages are scans, which need OCR"),
    exit_code(4, "a PDF is encrypted with a password"),
    exit_code(5, "not a PDF, XLSX or DOCX: legacy .xls and .doc, .xlsb, presentations"),
    exit_code(6, "a limit was hit: size, decompressed size, nesting or time"),
    after_long_help = EXIT_STATUS
)]
pub struct Imprimatur {
    /// One JSON object per document, each on its own line
    #[usage(long, global)]
    pub json: bool,

    #[usage(subcommand)]
    pub command: Commands,
}

#[derive(Subcommands, Debug)]
#[usage(run_with)]
pub enum Commands {
    #[usage(alias = "md")]
    Markdown(Markdown),
    Grep(Grep),
    Images(Images),
    #[usage(no_ctx)]
    Completion(Completion),
}
