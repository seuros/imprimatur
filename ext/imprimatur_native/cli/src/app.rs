//! What every command shares: the output mode, reading a document, writing
//! to standard output, and the exit status each kind of failure maps to.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, StdoutLock, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use imprimatur::read::{Content, Limits};

/// The context handed to commands.
#[derive(Debug, Clone, Copy)]
pub struct App {
    /// One JSON object per document instead of text.
    pub json: bool,
}

/// How a document failed, as scripts branch on it: exit status and the
/// name `--json` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Damaged,
    NoTextLayer,
    PasswordRequired,
    Unsupported,
    LimitExceeded,
}

impl Failure {
    pub const fn of(error: &imprimatur::Error) -> Self {
        match error {
            imprimatur::Error::NoTextLayer => Self::NoTextLayer,
            imprimatur::Error::PasswordRequired => Self::PasswordRequired,
            imprimatur::Error::UnsupportedDocument(_) => Self::Unsupported,
            imprimatur::Error::LimitExceeded(_) | imprimatur::Error::Cancelled => {
                Self::LimitExceeded
            }
            _ => Self::Damaged,
        }
    }

    /// The exit status, as `imprimatur --help` lists them.
    pub const fn code(self) -> u8 {
        match self {
            Self::Damaged => 1,
            Self::NoTextLayer => 3,
            Self::PasswordRequired => 4,
            Self::Unsupported => 5,
            Self::LimitExceeded => 6,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Damaged => "damaged",
            Self::NoTextLayer => "no_text_layer",
            Self::PasswordRequired => "password_required",
            Self::Unsupported => "unsupported",
            Self::LimitExceeded => "limit_exceeded",
        }
    }
}

/// The exit status of a run: success, or the first failure met.
#[derive(Debug, Default, Clone, Copy)]
pub struct Status(Option<Failure>);

impl Status {
    pub fn record(&mut self, failure: Failure) {
        self.0.get_or_insert(failure);
    }

    pub fn exit_code(self) -> ExitCode {
        self.0.map_or(ExitCode::SUCCESS, |failure| ExitCode::from(failure.code()))
    }
}

/// The document's bytes: the file, or standard input for `-`.
pub fn read(file: &Path) -> imprimatur::Result<Vec<u8>> {
    if file.as_os_str() == "-" {
        let mut document = Vec::new();
        io::stdin().lock().read_to_end(&mut document)?;
        return Ok(document);
    }
    Ok(fs::read(file)?)
}

/// A document's content: the file read, then the document.
pub fn content(file: &Path, limits: &Limits) -> imprimatur::Result<Content> {
    read(file).and_then(|document| imprimatur::read::content(&document, limits))
}

/// What a document's outputs are named after: its file's stem, `stdin`
/// for `-`.
pub fn stem(file: &Path) -> Cow<'_, str> {
    if file.as_os_str() == "-" {
        return "stdin".into();
    }
    file.file_stem().map_or_else(|| "document".into(), |stem| stem.to_string_lossy())
}

/// `DIRECTORY/BASE.EXTENSION`, or `BASE-2`, `BASE-3`... when this run
/// already wrote that path.
pub fn unique_path(
    directory: &Path,
    base: &str,
    extension: &str,
    written: &mut HashSet<PathBuf>,
) -> PathBuf {
    let mut target = directory.join(format!("{base}.{extension}"));
    let mut number = 2;
    while !written.insert(target.clone()) {
        target = directory.join(format!("{base}-{number}.{extension}"));
        number += 1;
    }
    target
}

/// Creates the directory outputs go to, or reports why it cannot be and
/// gives the exit status.
pub fn create_dir(directory: &Path) -> Result<(), ExitCode> {
    fs::create_dir_all(directory).map_err(|error| {
        eprintln!("imprimatur: {}: {error}", directory.display());
        ExitCode::from(Failure::Damaged.code())
    })
}

/// Standard output. A closed pipe (`| head`) is not an error: writes after
/// it report `false`, and the command stops quietly.
pub struct Out {
    stdout: StdoutLock<'static>,
}

impl Out {
    pub fn new() -> Self {
        Self { stdout: io::stdout().lock() }
    }

    /// Whether the text could be written.
    pub fn write(&mut self, text: &str) -> bool {
        self.stdout.write_all(text.as_bytes()).is_ok()
    }

    /// One JSON value on a line.
    pub fn json(&mut self, value: &serde_json::Value) -> bool {
        let mut line = value.to_string();
        line.push('\n');
        self.write(&line)
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        let _ = self.stdout.flush(); // Nothing left to tell a closed pipe.
    }
}

impl App {
    /// Reports a document that failed: on standard error, or as a JSON line
    /// with `--json`. Returns the failure, and whether output still works.
    pub fn report_failure(
        self,
        out: &mut Out,
        file: &Path,
        error: &imprimatur::Error,
    ) -> (Failure, bool) {
        let failure = Failure::of(error);
        let open = if self.json {
            out.json(&serde_json::json!({
                "file": file.to_string_lossy(),
                "ok": false,
                "error": failure.name(),
                "message": error.to_string(),
            }))
        } else {
            eprintln!("imprimatur: {}: {error}", file.display());
            true
        };
        (failure, open)
    }
}
