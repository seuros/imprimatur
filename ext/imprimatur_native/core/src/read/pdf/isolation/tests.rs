use std::assert_matches;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::errno::Errno;
use nix::sys::wait::waitpid;
use nix::unistd::Pid;

use super::test_support::{Worker, page, reply};
use super::*;
use crate::read::pdf::Status;

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
            Err(Error::InvalidPdfOption(_))
        );
    }
}

#[test]
fn worker_timeout_keeps_flushed_pages_but_cancellation_still_raises() {
    let worker = Worker::new(&format!(
        "{}\nexec /bin/sleep 30",
        reply(3, vec![page(1, Status::Text, "Completed before the deadline")], None)
    ));
    let process =
        ProcessLimits { wall_time: Duration::from_millis(250), ..ProcessLimits::default() };
    let started = Instant::now();
    let result =
        pages_isolated(&worker.path, b"", &ReadOptions::default(), &Limits::default(), &process)
            .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "the original deadline was not renewed");
    assert_eq!(result.info.page_count, 3);
    assert_eq!(result.pages[0].text, "Completed before the deadline");
    assert_eq!(
        result.pages.iter().map(|page| page.status).collect::<Vec<_>>(),
        [Status::Text, Status::Skipped, Status::Skipped]
    );

    let flag = Arc::new(AtomicBool::new(false));
    let limits = Limits::default().cancel_on(Arc::clone(&flag));
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        flag.store(true, Ordering::Relaxed);
    });
    assert_matches!(
        pages_isolated(&worker.path, b"", &ReadOptions::default(), &limits, &process),
        Err(Error::Cancelled)
    );
    cancel.join().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn monitor_errors_allow_an_exiting_worker_but_fail_closed_for_a_live_one() {
    let limits = Limits::default();
    let budget = Budget::new(&limits, 0).unwrap();
    let mut child = sleeper();
    let pid = child.child.id();
    child.child.kill().unwrap();
    let missing = || Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
    let status = finish_monitor_error(
        &mut child,
        &budget,
        Instant::now() + Duration::from_secs(1),
        missing(),
    )
    .unwrap();
    assert!(!status.success());
    drop(child);
    assert_reaped(pid);

    let mut child = sleeper();
    let pid = child.child.id();
    assert_matches!(
        finish_monitor_error(
            &mut child,
            &budget,
            Instant::now() + Duration::from_secs(1),
            missing()
        ),
        Err(Error::Io(_))
    );
    drop(child);
    assert_reaped(pid);
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
