//! The companion worker sets limits before touching the document.

use std::io::Read;
use std::process::ExitCode;

use super::{protocol, system};
use crate::read::pdf::render_page;
use crate::{Error, Result};

/// Internal entry point for the bundled worker executable.
#[doc(hidden)]
#[must_use]
pub fn worker_main() -> ExitCode {
    match protocol::write_reply(std::io::stdout().lock(), run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("imprimatur render worker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Vec<u8>> {
    let request = protocol::Request::parse()?;
    let output = request
        .options
        .max_output_bytes
        .max(protocol::MAX_ERROR_BYTES)
        .checked_add(protocol::HEADER.len())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(Error::InvalidPdfOption("output limit is too large"))?;
    system::apply_limits(request.memory, request.cpu_seconds, output)?;
    let mut bytes = Vec::new();
    let maximum = u64::try_from(request.limits.max_input)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::InvalidPdfOption("input limit is too large"))?;
    std::io::stdin().lock().take(maximum).read_to_end(&mut bytes)?;
    if bytes.len() > request.limits.max_input {
        return Err(Error::LimitExceeded("input size"));
    }
    render_page(&bytes, request.number, &request.options, &request.limits).map(|image| image.data)
}
