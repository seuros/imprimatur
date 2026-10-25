//! Apply resource limits before parsing any document bytes.

use std::io::{Read, Write};
use std::process::ExitCode;

use super::protocol::{Operation, Request};
use super::{protocol, stream, system};
use crate::read::pdf::{self, bounded};
use crate::{Error, Result};

/// Internal entry point for the bundled PDF worker executable.
#[doc(hidden)]
#[must_use]
pub fn worker_main() -> ExitCode {
    let mut out = std::io::stdout().lock();
    let result = Request::parse().and_then(|request| run(&request, &mut out));
    match result.or_else(|error| protocol::write_reply(&mut out, Err(error))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("imprimatur PDF worker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(request: &Request, out: &mut impl Write) -> Result<()> {
    let output = u64::try_from(request.operation.maximum()?)
        .map_err(|_| Error::InvalidPdfOption("worker output limit is too large"))?;
    system::apply_limits(request.memory, request.cpu_seconds, output)?;
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if !request.restricted {
        system::reexec(&request.arguments())?;
    }
    let maximum = u64::try_from(request.limits.max_input)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::InvalidPdfOption("input limit is too large"))?;
    let mut bytes = Vec::new();
    std::io::stdin().lock().take(maximum).read_to_end(&mut bytes)?;
    if bytes.len() > request.limits.max_input {
        return Err(Error::LimitExceeded("input size"));
    }
    match &request.operation {
        Operation::Info => protocol::write_reply(
            out,
            pdf::info(&bytes, &request.limits).and_then(|info| {
                serde_json::to_vec(&info)
                    .map_err(|error| Error::malformed("PDF worker", error.to_string()))
            }),
        ),
        Operation::Render(number, options) => protocol::write_reply(
            out,
            pdf::render_page(&bytes, *number, options, &request.limits).map(|image| image.data),
        ),
        Operation::Pages(options) => {
            out.write_all(protocol::HEADER)?;
            let result = pdf::info(&bytes, &request.limits).and_then(|info| {
                stream::write(out, &stream::Frame::Info(info))?;
                bounded::visit_pages(&bytes, options, &request.limits, true, |page| {
                    stream::write(out, &stream::Frame::Page(page))
                })?;
                Ok(())
            });
            let frame = match result {
                Ok(()) => stream::Frame::End,
                Err(error) => {
                    let (kind, message) = protocol::failure(&error);
                    stream::Frame::Error { kind, message }
                }
            };
            stream::write(out, &frame)
        }
    }
}
