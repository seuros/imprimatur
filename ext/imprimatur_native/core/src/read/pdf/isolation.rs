//! Render in a separate, resource-limited process. No untrusted PDF parsing
//! occurs in the supervisor.

mod protocol;
mod system;
#[cfg(test)]
mod tests;
mod worker;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::RenderOptions;
use crate::read::{Budget, Image, Limits};
use crate::{Error, Result};
pub use worker::worker_main;

/// Mandatory process limits, in addition to the reader/render limits.
#[derive(Debug, Clone)]
pub struct ProcessLimits {
    /// Address-space cap on Linux/FreeBSD; RSS watchdog threshold on macOS.
    pub memory_bytes: u64,
    /// A finite upper bound even when the reader's timeout is disabled.
    pub wall_time: Duration,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self { memory_bytes: 1024 * 1024 * 1024, wall_time: Duration::from_mins(1) }
    }
}

/// Renders through the explicitly selected, trusted worker executable.
///
/// The child is killed and reaped on timeout, cancellation, resource failure
/// or supervisor errors. Missing workers never fall back to in-process work.
/// macOS uses a sampled RSS watchdog and may briefly overshoot its threshold.
///
/// # Errors
/// Invalid limits, failed worker startup/protocol, render errors, or exceeded
/// input/output, process memory or wall-clock limits.
pub fn render_page_isolated(
    worker: &Path,
    data: &[u8],
    number: u32,
    options: &RenderOptions,
    limits: &Limits,
    process: &ProcessLimits,
) -> Result<Image> {
    let budget = Budget::new(limits, data.len())?;
    budget.check()?;
    let deadline = Instant::now()
        .checked_add(process.wall_time)
        .ok_or(Error::InvalidPdfOption("worker wall time is too large"))?;
    if !worker.is_absolute() || !worker.is_file() {
        return Err(Error::InvalidPdfOption("an absolute path to the render worker is required"));
    }
    if process.memory_bytes == 0 || process.wall_time.is_zero() || number == 0 {
        return Err(Error::InvalidPdfOption("process limits and page number must be positive"));
    }
    let request = protocol::Request::new(number, options, limits, process)?;
    let directory = tempfile::tempdir()?;
    let input_path = directory.path().join("input.pdf");
    File::create(&input_path)?.write_all(data)?;
    let output_path = directory.path().join("output");
    let error_path = directory.path().join("stderr");
    check(&budget, deadline)?;
    let child = Command::new(worker)
        .args(request.arguments())
        .stdin(Stdio::from(File::open(&input_path)?))
        .stdout(Stdio::from(File::create(&output_path)?))
        .stderr(Stdio::from(File::create(&error_path)?))
        .process_group(0)
        .spawn()?;
    let mut child = Running { child, finished: false };
    let status = supervise(&mut child, &budget, deadline, process.memory_bytes)?;
    if !status.success() {
        return Err(Error::LimitExceeded("render worker resources"));
    }
    let maximum = options
        .max_output_bytes
        .max(protocol::MAX_ERROR_BYTES)
        .checked_add(protocol::HEADER.len())
        .ok_or(Error::InvalidPdfOption("output limit is too large"))?;
    let maximum = u64::try_from(maximum).unwrap_or(u64::MAX);
    if fs::metadata(&output_path)?.len() > maximum {
        return Err(Error::LimitExceeded("render worker output"));
    }
    let mut output = Vec::new();
    File::open(output_path)?.take(maximum).read_to_end(&mut output)?;
    check_finished(&budget, deadline)?;
    let data = protocol::decode(output, options.max_output_bytes)?;
    // The trusted worker writes only PNG; also reject malformed/empty replies.
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") || data.len() < 26 {
        return Err(Error::malformed("render worker", "invalid PNG response"));
    }
    let dimension = |at| u32::from_be_bytes(data[at..at + 4].try_into().unwrap_or([0; 4]));
    let (width, height) = (dimension(16), dimension(20));
    options.image(data, number, (width, height))
}

fn check(budget: &Budget<'_>, deadline: Instant) -> Result<()> {
    budget.check()?;
    check_finished(budget, deadline)
}

fn check_finished(budget: &Budget<'_>, deadline: Instant) -> Result<()> {
    budget.check_time()?;
    if Instant::now() >= deadline {
        return Err(Error::LimitExceeded("render worker wall time"));
    }
    Ok(())
}

struct Running {
    child: Child,
    finished: bool,
}

impl Running {
    fn poll(&mut self) -> Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.finished = status.is_some();
        Ok(status)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.finished {
            system::terminate(&mut self.child);
        }
    }
}

fn supervise(
    child: &mut Running,
    budget: &Budget<'_>,
    deadline: Instant,
    memory: u64,
) -> Result<ExitStatus> {
    loop {
        // SIGCHLD can invoke Ruby's unblock callback. Prefer a completed
        // worker; the binding checks pending Ruby exceptions before returning.
        if let Some(status) = child.poll()? {
            return Ok(status);
        }
        check(budget, deadline)?;
        if let Err(error) = system::check_memory(child.child.id(), memory) {
            // It may have exited between try_wait and the memory query.
            if let Some(status) = child.poll()? {
                return Ok(status);
            }
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
