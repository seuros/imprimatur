use std::assert_matches;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::errno::Errno;
use nix::sys::wait::waitpid;
use nix::unistd::Pid;

use super::*;

fn sleeper() -> Running {
    Running {
        child: Command::new("/bin/sleep").arg("30").process_group(0).spawn().unwrap(),
        finished: false,
    }
}

fn assert_reaped(pid: u32) {
    assert_eq!(waitpid(Pid::from_raw(i32::try_from(pid).unwrap()), None), Err(Errno::ECHILD));
}

#[test]
fn rejects_invalid_requests_before_starting_a_worker() {
    let worker = std::env::current_exe().unwrap();
    let options = RenderOptions::default();
    let limits = Limits::default();
    let process = ProcessLimits::default();
    for (number, process) in [
        (0, process.clone()),
        (1, ProcessLimits { memory_bytes: 0, ..process.clone() }),
        (1, ProcessLimits { wall_time: Duration::ZERO, ..process }),
    ] {
        assert_matches!(
            render_page_isolated(&worker, b"", number, &options, &limits, &process),
            Err(Error::InvalidPdfOption("process limits and page number must be positive"))
        );
    }
}

#[test]
fn deadline_and_cancellation_kill_and_reap_the_worker() {
    let limits = Limits::default().timeout(None);
    let budget = Budget::new(&limits, 0).unwrap();
    let mut child = sleeper();
    let pid = child.child.id();
    assert_matches!(
        supervise(&mut child, &budget, Instant::now() + Duration::from_millis(30), u64::MAX),
        Err(Error::LimitExceeded("render worker wall time"))
    );
    drop(child);
    assert_reaped(pid);

    let flag = Arc::new(AtomicBool::new(false));
    let limits = limits.cancel_on(Arc::clone(&flag));
    let budget = Budget::new(&limits, 0).unwrap();
    let mut child = sleeper();
    let pid = child.child.id();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        flag.store(true, Ordering::Relaxed);
    });
    assert_matches!(
        supervise(&mut child, &budget, Instant::now() + Duration::from_secs(5), u64::MAX),
        Err(Error::Cancelled)
    );
    cancel.join().unwrap();
    drop(child);
    assert_reaped(pid);
}

#[cfg(target_os = "macos")]
#[test]
fn rss_watchdog_kills_and_reaps_an_over_budget_worker() {
    let limits = Limits::default();
    let budget = Budget::new(&limits, 0).unwrap();
    let mut child = sleeper();
    let pid = child.child.id();
    assert_matches!(
        supervise(&mut child, &budget, Instant::now() + Duration::from_secs(5), 1),
        Err(Error::LimitExceeded("render worker memory"))
    );
    drop(child);
    assert_reaped(pid);
}
