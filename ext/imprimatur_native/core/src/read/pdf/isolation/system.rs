//! OS resource enforcement. macOS uses the supervisor's RSS watchdog.

#[cfg(test)]
mod tests;

use std::process::Child;

use nix::errno::Errno;
use nix::sys::resource::{Resource, getrlimit, rlim_t, setrlimit};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::{Error, Result};

pub(super) fn apply_limits(memory: u64, cpu: u64, output: u64) -> Result<()> {
    if memory == 0 {
        return Err(Error::InvalidPdfOption("memory limit must be positive"));
    }
    #[cfg(target_os = "linux")]
    lower(Resource::RLIMIT_AS, memory)?;
    #[cfg(target_os = "freebsd")]
    lower(Resource::RLIMIT_VMEM, memory)?;
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "macos")))]
    return Err(Error::UnsupportedDocument("process limits are unsupported on this OS".into()));
    lower(Resource::RLIMIT_CORE, 0)?;
    lower(Resource::RLIMIT_CPU, cpu)?;
    lower(Resource::RLIMIT_FSIZE, output)?;
    Ok(())
}

/// Replace the bootstrap after setting address-space limits, so the loader
/// and parser cannot reuse memory mapped before the cap was installed.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(super) fn reexec(arguments: &[std::ffi::OsString]) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let string = |value: &[u8]| {
        CString::new(value)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
    };
    let executable = string(std::env::current_exe()?.as_os_str().as_bytes())?;
    let mut args = vec![executable.clone(), string(b"1")?];
    for argument in arguments.iter().skip(1) {
        args.push(string(argument.as_os_str().as_bytes())?);
    }
    match nix::unistd::execv(&executable, &args) {
        Ok(never) => match never {},
        // A tiny address-space cap can prevent exec from allocating its
        // argument/environment pages and surface as E2BIG rather than ENOMEM.
        Err(Errno::ENOMEM | Errno::E2BIG) => {
            Err(Error::LimitExceeded("PDF worker startup resources"))
        }
        Err(error) => Err(std::io::Error::from(error).into()),
    }
}

fn lower(resource: Resource, value: u64) -> Result<()> {
    let (soft, hard) = getrlimit(resource).map_err(std::io::Error::from)?;
    let value = bounded_limit(value, soft, hard)?;
    setrlimit(resource, value, value).map_err(std::io::Error::from)?;
    Ok(())
}

fn bounded_limit(value: u64, soft: rlim_t, hard: rlim_t) -> Result<rlim_t> {
    let value = i128::from(value).min(i128::from(soft)).min(i128::from(hard));
    value.try_into().map_err(|_| Error::InvalidPdfOption("resource limit is out of range"))
}

#[cfg(target_os = "macos")]
pub(super) fn check_memory(pid: u32, maximum: u64) -> Result<()> {
    let pid = i32::try_from(pid).map_err(|_| Error::InvalidPdfOption("invalid worker PID"))?;
    let usage = libproc::proc_pid::pidinfo::<libproc::task_info::TaskInfo>(pid, 0)
        .map_err(|error| std::io::Error::other(format!("cannot monitor render worker: {error}")))?;
    if usage.pti_resident_size > maximum {
        return Err(Error::LimitExceeded("render worker memory"));
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
