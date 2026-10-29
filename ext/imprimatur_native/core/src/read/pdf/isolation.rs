//! Read and render in a resource-limited process. No untrusted PDF parsing
//! occurs in the supervisor.

mod protocol;
mod recovery;
mod stream;
mod system;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod worker;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::{Info, Pages, ReadOptions, RenderOptions};
use crate::read::{Budget, Image, Limits};
use crate::{Error, Result};
pub use recovery::pages_isolated_recovering;
pub use worker::worker_main;

/// Inspects a PDF in the trusted, resource-limited worker.
///
/// # Errors
/// Invalid options, worker errors, or exceeded input, memory or time limits.
pub fn info_isolated(
    worker: &Path,
    data: &[u8],
    limits: &Limits,
    process: &ProcessLimits,
) -> Result<Info> {
    let output = invoke(worker, data, protocol::Operation::Info, limits, process)?;
    let bytes = output.complete(protocol::MAX_ERROR_BYTES)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| Error::malformed("PDF worker", error.to_string()))
}

/// Reads PDF pages in the worker, retaining completed records on resource failure.
///
/// Page-local stream limits trigger bounded individual recovery.
/// All attempts share one external wall-clock and process-memory limit.
///
/// # Errors
/// As [`info_isolated`], and cancellation or an invalid worker reply.
/// A failure before page metadata is available remains a document error.
pub fn pages_isolated(
    worker: &Path,
    data: &[u8],
    options: &ReadOptions,
    limits: &Limits,
    process: &ProcessLimits,
) -> Result<Pages> {
    let output =
        invoke(worker, data, protocol::Operation::Pages(options.clone()), limits, process)?;
    stream::decode(output, options)
}

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

impl ProcessLimits {
    fn wall_time_for(&self, limits: &Limits) -> Duration {
        limits.timeout.unwrap_or(self.wall_time).min(self.wall_time)
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
    let output = invoke(
        worker,
        data,
        protocol::Operation::Render(number, options.clone()),
        limits,
        process,
    )?;
    let data = output.complete(options.max_output_bytes)?;
    // The trusted worker writes only PNG; reject malformed/empty replies too.
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") || data.len() < 26 {
        return Err(Error::malformed("PDF worker", "invalid PNG response"));
    }
    let dimension = |at| u32::from_be_bytes(data[at..at + 4].try_into().unwrap_or([0; 4]));
    let (width, height) = (dimension(16), dimension(20));
    options.image(data, number, (width, height))
}

struct Output {
    data: Vec<u8>,
    failure: Option<Error>,
}

impl Output {
    fn complete(self, maximum: usize) -> Result<Vec<u8>> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        protocol::decode(self.data, maximum)
    }
}

fn invoke(
    worker: &Path,
    data: &[u8],
    operation: protocol::Operation,
    limits: &Limits,
    process: &ProcessLimits,
) -> Result<Output> {
    let budget = Budget::new(limits, data.len())?;
    budget.check()?;
    let deadline = Instant::now()
        .checked_add(process.wall_time)
        .ok_or(Error::InvalidPdfOption("worker wall time is too large"))?;
    if !worker.is_absolute() || !worker.is_file() {
        return Err(Error::InvalidPdfOption("an absolute path to the render worker is required"));
    }
    if process.memory_bytes == 0 || process.wall_time.is_zero() {
        return Err(Error::InvalidPdfOption("process limits must be positive"));
    }
    let request = protocol::Request::new(operation, limits, process)?;
    let maximum = u64::try_from(request.operation.maximum()?)
        .map_err(|_| Error::InvalidPdfOption("worker output limit is too large"))?;
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
    let failure = match supervise(&mut child, &budget, deadline, process.memory_bytes) {
        Ok(status) if status.success() => check_finished(&budget, deadline).err(),
        Ok(_) => Some(Error::LimitExceeded("PDF worker resources")),
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => Some(error),
    };
    // Stop and reap an unfinished worker before inspecting its flushed output.
    drop(child);
    if fs::metadata(&output_path)?.len() > maximum {
        return Err(Error::LimitExceeded("PDF worker output"));
    }
    let mut output = Vec::new();
    File::open(output_path)?.take(maximum).read_to_end(&mut output)?;
    Ok(Output { data: output, failure })
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
    #[cfg(not(target_os = "macos"))]
    let _ = memory;

    loop {
        // SIGCHLD can invoke Ruby's unblock callback. Prefer a completed
        // worker; the binding checks pending Ruby exceptions before returning.
        if let Some(status) = child.poll()? {
            return Ok(status);
        }
        check(budget, deadline)?;
        #[cfg(target_os = "macos")]
        if let Err(error) = system::check_memory(child.child.id(), memory) {
            if matches!(error, Error::Io(_)) {
                return finish_monitor_error(child, budget, deadline, error);
            }
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(target_os = "macos")]
fn finish_monitor_error(
    child: &mut Running,
    budget: &Budget<'_>,
    deadline: Instant,
    error: Error,
) -> Result<ExitStatus> {
    // proc_pidinfo can stop seeing an exiting task before waitpid can reap it.
    // Never bypass monitoring for a live worker: allow only a bounded exit
    // grace, still respecting cancellation and the original overall deadline.
    let grace = Instant::now() + Duration::from_millis(50);
    loop {
        if let Some(status) = child.poll()? {
            return Ok(status);
        }
        check(budget, deadline)?;
        if Instant::now() >= grace {
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
