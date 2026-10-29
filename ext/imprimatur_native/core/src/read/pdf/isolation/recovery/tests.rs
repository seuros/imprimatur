use std::assert_matches;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::read::pdf::PageResult;
use crate::read::pdf::isolation::stream::Frame;
use crate::read::pdf::isolation::test_support::{Worker, page, reply};

const ATTEMPT: Duration = Duration::from_millis(150);

fn recover(worker: &Worker, options: &ReadOptions) -> Result<Pages> {
    recover_within(worker, options, &Limits::default(), Duration::from_secs(5))
}

fn recover_within(
    worker: &Worker,
    options: &ReadOptions,
    limits: &Limits,
    total: Duration,
) -> Result<Pages> {
    let process = ProcessLimits { wall_time: total, ..ProcessLimits::default() };
    pages_isolated_recovering(&worker.path, b"", options, limits, &process, ATTEMPT)
}

fn options(last_page: u32) -> ReadOptions {
    ReadOptions { last_page, ..ReadOptions::default() }
}

fn single(count: u32, page: PageResult) -> String {
    reply(count, vec![page], Some(Frame::End))
}

fn outcomes(result: &Pages) -> Vec<(u32, Status, &str)> {
    result.pages.iter().map(|page| (page.number, page.status, page.text.as_str())).collect()
}

#[test]
fn only_the_structured_timeout_kind_triggers_recovery() {
    let cap = ReadOptions::default().max_output_bytes;
    let recovered =
        vec![format!("1:3:{cap}"), format!("3:3:{}", cap - 4), format!("2:2:{}", cap - 13)];
    for (kind, attempts, statuses) in [
        (6, recovered, [Status::Text; 3]),
        (3, vec![format!("1:3:{cap}")], [Status::Text, Status::Skipped, Status::Skipped]),
    ] {
        let error = Frame::Error { kind, message: "the document exceeds the time limit".into() };
        let batch = reply(3, vec![page(1, Status::Text, "kept")], Some(error));
        let worker = Worker::recovering(
            &format!("{batch}\nexit 0"),
            &[
                ("3", single(3, page(3, Status::Text, "neighbour"))),
                ("2", single(3, page(2, Status::Text, "deferred"))),
            ],
        );
        let result = recover(&worker, &options(3)).unwrap();
        assert_eq!(worker.attempts(), attempts, "kind {kind}");
        assert_eq!(result.pages.iter().map(|p| p.status).collect::<Vec<_>>(), statuses);
    }
}

#[test]
fn recovery_keeps_completed_and_page_local_outcomes_trying_neighbours_first() {
    let worker = Worker::recovering(
        &reply(7, vec![page(1, Status::Text, "kept"), page(2, Status::Empty, "")], None),
        &[
            ("4", "exec /bin/sleep 30".into()),
            ("5", single(7, page(5, Status::Failed, ""))),
            ("6", single(7, page(6, Status::Text, "later"))),
            ("7", single(7, page(7, Status::Empty, ""))),
            ("3", single(7, page(3, Status::Text, "recovered stall"))),
        ],
    );
    let result = recover(&worker, &options(7)).unwrap();
    let cap = ReadOptions::default().max_output_bytes;
    assert_eq!(
        worker.attempts(),
        [
            format!("1:7:{cap}"),
            format!("4:4:{}", cap - 4),
            format!("5:5:{}", cap - 4),
            format!("6:6:{}", cap - 4),
            format!("7:7:{}", cap - 9),
            format!("3:3:{}", cap - 9),
        ]
    );
    assert_eq!(
        outcomes(&result),
        [
            (1, Status::Text, "kept"),
            (2, Status::Empty, ""),
            (3, Status::Text, "recovered stall"),
            (4, Status::Skipped, ""),
            (5, Status::Failed, ""),
            (6, Status::Text, "later"),
            (7, Status::Empty, ""),
        ]
    );
    assert_eq!(result.pages[4].reason.as_deref(), Some("page-local"));
}

#[test]
fn recovery_passes_only_remaining_output_and_stops_when_it_is_spent() {
    let mut tail = page(3, Status::Text, "xyz");
    tail.truncated = true;
    tail.reason = Some("the page text limit was reached".into());
    let worker = Worker::recovering(
        &reply(4, vec![page(1, Status::Text, "keep")], None),
        &[("3", single(4, tail))],
    );
    let result = recover(&worker, &ReadOptions { max_output_bytes: 7, ..options(4) }).unwrap();
    assert_eq!(worker.attempts(), ["1:4:7", "3:3:3"]);
    assert_eq!(
        outcomes(&result),
        [
            (1, Status::Text, "keep"),
            (2, Status::Skipped, ""),
            (3, Status::Text, "xyz"),
            (4, Status::Skipped, ""),
        ]
    );
    assert!(result.pages[2].truncated);
}

#[test]
fn completed_output_limit_skips_are_not_retry_candidates() {
    let worker = Worker::new(&reply(
        3,
        vec![
            page(1, Status::Text, "full"),
            bounded::unread(2, Status::Skipped, "the text output limit was reached".into()),
            bounded::unread(3, Status::Skipped, "the text output limit was reached".into()),
        ],
        Some(Frame::End),
    ));
    let result = recover(&worker, &ReadOptions { max_output_bytes: 4, ..options(3) }).unwrap();
    assert_eq!(worker.attempts(), ["1:3:4"]);
    assert_eq!(result.pages[1].status, Status::Skipped);
    assert_eq!(result.pages[2].status, Status::Skipped);
}

#[test]
fn total_deadline_bounds_all_attempts_and_retains_the_prefix() {
    let worker = Worker::recovering(
        &reply(20, vec![page(1, Status::Text, "kept")], None),
        &[("*", format!("{}\nexec /bin/sleep 30", reply(20, vec![], None)))],
    );
    let total = Duration::from_millis(550);
    let start = Instant::now();
    let result = recover_within(&worker, &options(20), &Limits::default(), total).unwrap();
    assert!(start.elapsed() < total + Duration::from_secs(1), "overall deadline was renewed");
    assert_eq!(result.pages.len(), 20);
    assert_eq!(result.pages[0].text, "kept");
    let attempts = worker.attempts();
    assert!(attempts.len() >= 2, "test did not exercise a recovery attempt");
    assert!(attempts.len() <= 5, "too many attempts for the original total deadline: {attempts:?}");
}

#[test]
fn cancellation_during_recovery_propagates_instead_of_returning_partial_pages() {
    let worker = Worker::recovering(
        &reply(3, vec![page(1, Status::Text, "kept")], None),
        &[("*", format!("{}\n: > singleton_started\nexec /bin/sleep 30", reply(3, vec![], None)))],
    );
    let flag = Arc::new(AtomicBool::new(false));
    let limits = Limits::default().cancel_on(Arc::clone(&flag));
    let marker = worker.directory.path().join("singleton_started");
    let cancel = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = marker.exists();
        flag.store(true, Ordering::Relaxed);
        started
    });
    let result = recover_within(&worker, &options(3), &limits, Duration::from_secs(5));
    assert!(cancel.join().unwrap(), "cancellation was not exercised during recovery");
    assert_matches!(result, Err(Error::Cancelled));
}

#[test]
fn malformed_singleton_protocol_is_fatal() {
    let worker = Worker::recovering(
        &reply(3, vec![page(1, Status::Text, "kept")], None),
        &[("3", format!("{}\nprintf 'not-json\\n'", reply(3, vec![], None)))],
    );
    assert_matches!(recover(&worker, &options(3)), Err(Error::Malformed { .. }));
    assert_eq!(worker.attempts().len(), 2, "fatal protocol error must stop scheduling");
}

#[test]
fn singleton_spawn_io_failure_is_fatal() {
    let worker = Worker::new(&format!(
        "{}\nchmod 600 \"$0\" || exit 1\nexec /bin/sleep 30",
        reply(3, vec![page(1, Status::Text, "kept")], None),
    ));
    assert_matches!(recover(&worker, &options(3)), Err(Error::Io(_)));
    assert_eq!(worker.attempts().len(), 1);
}
