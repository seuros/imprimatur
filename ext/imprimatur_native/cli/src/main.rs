//! `imprimatur`: documents in, Markdown out. Reads PDF, XLSX and DOCX with
//! the imprimatur crate; see `imprimatur --help`.

mod app;
mod cli;

use std::process::ExitCode;

use usage_rs::RunWith;

fn main() -> ExitCode {
    let cli = cli::Imprimatur::parse();
    let app = app::App { json: cli.json };
    cli.command.run_with(&app)
}
