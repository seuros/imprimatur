//! OS resource enforcement. macOS uses the supervisor's RSS watchdog.

use std::process::Child;

use nix::errno::Errno;
use nix::sys::resource::{Resource, getrlimit, setrlimit};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::{Error, Result};

pub(super) fn apply_limits(memory: u64, cpu: u64, output: u64) -> Result<()> {
    if memory == 0 {
        return Err(Error::InvalidPdfOption("memory limit must be positive"));
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    lower(Resource::RLIMIT_AS, memory)?;
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "macos")))]
    return Err(Error::UnsupportedDocument("process limits are unsupported on this OS".into()));
    lower(Resource::RLIMIT_CORE, 0)?;
    lower(Resource::RLIMIT_CPU, cpu)?;
    lower(Resource::RLIMIT_FSIZE, output)?;
    Ok(())
}

fn lower(resource: Resource, value: u64) -> Result<()> {
    let (soft, hard) = getrlimit(resource).map_err(std::io::Error::from)?;
    let value = value.min(soft).min(hard);
    setrlimit(resource, value, value).map_err(std::io::Error::from)?;
    Ok(())
}

pub(super) fn check_memory(pid: u32, maximum: u64) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let pid = i32::try_from(pid).map_err(|_| Error::InvalidPdfOption("invalid worker PID"))?;
        let usage = libproc::proc_pid::pidinfo::<libproc::task_info::TaskInfo>(pid, 0).map_err(
            |error| std::io::Error::other(format!("cannot monitor render worker: {error}")),
        )?;
        if usage.pti_resident_size > maximum {
            return Err(Error::LimitExceeded("render worker memory"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Memory is enforced by RLIMIT_AS inside the worker.
        let _ = (pid, maximum);
    }
    Ok(())
}

pub(super) fn terminate(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id())
        && let Err(error) = killpg(Pid::from_raw(pid), Signal::SIGKILL)
        && error != Errno::ESRCH
    {
        eprintln!("imprimatur: could not kill render worker group: {error}");
    }
    // Also handle a child that failed before joining its new process group.
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
        && error.raw_os_error() != Some(Errno::ESRCH as i32)
    {
        eprintln!("imprimatur: could not kill render worker: {error}");
    }
    if let Err(error) = child.wait() {
        eprintln!("imprimatur: could not reap render worker: {error}");
    }
}
