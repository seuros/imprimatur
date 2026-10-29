//! Supervisor recovery tests: the trusted fake worker controls exactly which
//! records are flushed, without depending on a PDF that happens to parse slowly.

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::read::pdf::{PageResult, Status, bounded};

const ATTEMPT: Duration = Duration::from_millis(150);

struct Worker {
    directory: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl Worker {
    fn new(body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("worker");
        // $9 is first_page, ${10} last_page, ${13} max_output_bytes.
        // Relative logs avoid shell quoting temporary directory paths.
        let script = format!(
            "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n\
             printf '%s:%s:%s\\n' \"$9\" \"${{10}}\" \"${{13}}\" >> attempts\n{body}\n"
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self { directory, path }
    }

    fn attempts(&self) -> Vec<String> {
        match fs::read_to_string(self.directory.path().join("attempts")) {
            Ok(log) => log.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read worker attempt log: {error}"),
        }
    }

    fn run(&self, options: &ReadOptions, limits: &Limits, total: Duration) -> Result<Pages> {
        pages_isolated_recovering(
            &self.path,
            b"",
            options,
            limits,
            &ProcessLimits { wall_time: total, ..ProcessLimits::default() },
            ATTEMPT,
        )
    }
}

fn page(number: u32, status: Status, text: &str) -> PageResult {
    PageResult {
        number,
        status,
        text: text.into(),
        images: matches!(status, Status::Text | Status::Empty).then_some(0),
        truncated: false,
        reason: matches!(status, Status::Failed | Status::Skipped).then(|| "page-local".into()),
    }
}

fn response(count: u32, pages: Vec<PageResult>, end: bool) -> String {
    let mut bytes = protocol::HEADER.to_vec();
    stream::write(&mut bytes, &stream::Frame::Info(Info { page_count: count, encrypted: false }))
        .unwrap();
    for page in pages {
        stream::write(&mut bytes, &stream::Frame::Page(page)).unwrap();
    }
    if end {
        stream::write(&mut bytes, &stream::Frame::End).unwrap();
    }
    printf(&bytes)
}

fn printf(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for byte in bytes {
        write!(&mut escaped, "\\{byte:03o}").unwrap();
    }
    format!("printf '{escaped}'")
}

fn options(last_page: u32) -> ReadOptions {
    ReadOptions { last_page, ..ReadOptions::default() }
}

#[test]
fn structured_cooperative_timeout_kind_six_triggers_neighbours_first_recovery() {
    let mut error = Vec::new();
    stream::write(
        &mut error,
        &stream::Frame::Error { kind: 6, message: "the document exceeds the time limit".into() },
    )
    .unwrap();
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\n{}\nexit 0\nfi\n\
         case \"$9\" in\n3) {};;\n2) {};;\n*) exit 99;;\nesac",
        response(3, vec![page(1, Status::Text, "kept")], false),
        printf(&error),
        response(3, vec![page(3, Status::Text, "neighbour")], true),
        response(3, vec![page(2, Status::Text, "deferred")], true),
    ));
    let result = worker.run(&options(3), &Limits::default(), Duration::from_secs(5)).unwrap();
    let cap = ReadOptions::default().max_output_bytes;
    assert_eq!(
        worker.attempts(),
        [format!("1:3:{cap}"), format!("3:3:{}", cap - 4), format!("2:2:{}", cap - 13),]
    );
    assert_eq!(
        result.pages.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
        ["kept", "deferred", "neighbour"]
    );
}

#[test]
fn structured_resource_kind_three_with_metadata_does_not_trigger_recovery() {
    let mut error = Vec::new();
    stream::write(
        &mut error,
        &stream::Frame::Error {
            kind: 3,
            // Deliberately timeout-like wording: the numeric kind, not the
            // human-readable message, must control recovery eligibility.
            message: "the document exceeds the time limit".into(),
        },
    )
    .unwrap();
    let worker = Worker::new(&format!(
        "{}\n{}",
        response(3, vec![page(1, Status::Text, "kept")], false),
        printf(&error),
    ));
    let result = worker.run(&options(3), &Limits::default(), Duration::from_secs(5)).unwrap();
    assert_eq!(worker.attempts(), [format!("1:3:{}", ReadOptions::default().max_output_bytes)]);
    assert_eq!(result.pages[0].text, "kept");
    assert_eq!(result.pages[1].status, Status::Skipped);
    assert_eq!(result.pages[2].status, Status::Skipped);
}

#[test]
fn recovery_preserves_completed_and_empty_pages_and_tries_neighbours_first() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nexec /bin/sleep 30\nfi\n\
         case \"$9\" in\n\
         3) {};;\n4) {};;\n5) {};;\n\
         *) exit 99;;\nesac",
        response(5, vec![page(1, Status::Text, "kept"), page(2, Status::Empty, "")], false),
        response(5, vec![page(3, Status::Text, "recovered stall")], true),
        response(5, vec![page(4, Status::Text, "later")], true),
        response(5, vec![page(5, Status::Empty, "")], true),
    ));
    let result = worker.run(&options(5), &Limits::default(), Duration::from_secs(5)).unwrap();
    let cap = ReadOptions::default().max_output_bytes;
    assert_eq!(
        worker.attempts(),
        [
            format!("1:5:{cap}"),
            format!("4:4:{}", cap - 4),
            format!("5:5:{}", cap - 9),
            format!("3:3:{}", cap - 9),
        ]
    );
    assert_eq!(
        result.pages.iter().map(|p| (p.number, p.status, p.text.as_str())).collect::<Vec<_>>(),
        [
            (1, Status::Text, "kept"),
            (2, Status::Empty, ""),
            (3, Status::Text, "recovered stall"),
            (4, Status::Text, "later"),
            (5, Status::Empty, ""),
        ]
    );
}

#[test]
fn recovery_passes_only_remaining_output_and_stops_when_it_is_spent() {
    let mut tail = page(3, Status::Text, "xyz");
    tail.truncated = true;
    tail.reason = Some("the page text limit was reached".into());
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nexec /bin/sleep 30\nfi\n\
         if [ \"$9\" = 3 ] && [ \"${{13}}\" = 3 ]; then\n{}\nelse\nexit 99\nfi",
        response(4, vec![page(1, Status::Text, "keep")], false),
        response(4, vec![tail], true),
    ));
    let options = ReadOptions { max_output_bytes: 7, ..options(4) };
    let result = worker.run(&options, &Limits::default(), Duration::from_secs(5)).unwrap();
    assert_eq!(worker.attempts(), ["1:4:7", "3:3:3"]);
    assert_eq!(result.pages[0].text, "keep");
    assert_eq!(result.pages[2].text, "xyz");
    assert!(result.pages[2].truncated);
    assert_eq!(result.pages.iter().map(|p| p.text.len()).sum::<usize>(), 7);
    assert_eq!(result.pages[1].status, Status::Skipped);
    assert_eq!(result.pages[3].status, Status::Skipped);
}

#[test]
fn completed_output_limit_skips_are_not_retry_candidates() {
    let worker = Worker::new(&response(
        3,
        vec![
            page(1, Status::Text, "full"),
            bounded::unread(2, Status::Skipped, "the text output limit was reached".into()),
            bounded::unread(3, Status::Skipped, "the text output limit was reached".into()),
        ],
        true,
    ));
    let options = ReadOptions { max_output_bytes: 4, ..options(3) };
    let result = worker.run(&options, &Limits::default(), Duration::from_secs(5)).unwrap();
    assert_eq!(worker.attempts(), ["1:3:4"]);
    assert_eq!(result.pages[1].status, Status::Skipped);
    assert_eq!(result.pages[2].status, Status::Skipped);
}

#[test]
fn total_deadline_bounds_all_attempts_and_retains_the_prefix() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nelse\n{}\nfi\nexec /bin/sleep 30",
        response(20, vec![page(1, Status::Text, "kept")], false),
        response(20, vec![], false),
    ));
    let total = Duration::from_millis(550);
    let start = Instant::now();
    let result = worker.run(&options(20), &Limits::default(), total).unwrap();
    assert!(start.elapsed() < Duration::from_secs(2), "overall deadline was renewed");
    assert_eq!(result.pages.len(), 20);
    assert_eq!(result.pages[0].text, "kept");
    let attempts = worker.attempts();
    assert!(attempts.len() >= 2, "test did not exercise a recovery attempt");
    assert!(attempts.len() <= 5, "too many attempts for the original total deadline: {attempts:?}");
}

#[test]
fn cancellation_during_recovery_propagates_instead_of_returning_partial_pages() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nelse\n{}\n: > singleton_started\nfi\n\
         exec /bin/sleep 30",
        response(3, vec![page(1, Status::Text, "kept")], false),
        response(3, vec![], false),
    ));
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
    let result = worker.run(&options(3), &limits, Duration::from_secs(5));
    let singleton_started = cancel.join().unwrap();
    assert!(singleton_started, "cancellation was not exercised during recovery");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
}

#[test]
fn a_valid_singleton_page_failure_is_local_and_does_not_stop_recovery() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nexec /bin/sleep 30\nfi\n\
         case \"$9\" in\n3) {};;\n2) {};;\n*) exit 99;;\nesac",
        response(3, vec![page(1, Status::Text, "kept")], false),
        response(3, vec![page(3, Status::Failed, "")], true),
        response(3, vec![page(2, Status::Text, "retried")], true),
    ));
    let result = worker.run(&options(3), &Limits::default(), Duration::from_secs(5)).unwrap();
    assert_eq!(result.pages[0].text, "kept");
    assert_eq!(result.pages[1].text, "retried");
    assert_eq!(result.pages[2].status, Status::Failed);
    assert_eq!(result.pages[2].reason.as_deref(), Some("page-local"));
    assert_eq!(worker.attempts().len(), 3);
}

#[test]
fn a_retry_timeout_before_metadata_preserves_prefix_and_continues_to_next_neighbour() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nexec /bin/sleep 30\nfi\n\
         case \"$9\" in\n\
         3) exec /bin/sleep 30;;\n\
         4) {};;\n2) {};;\n*) exit 99;;\nesac",
        response(4, vec![page(1, Status::Text, "kept")], false),
        response(4, vec![page(4, Status::Text, "healthy neighbour")], true),
        response(4, vec![page(2, Status::Text, "deferred")], true),
    ));
    let result = worker.run(&options(4), &Limits::default(), Duration::from_secs(5)).unwrap();
    assert_eq!(
        worker
            .attempts()
            .iter()
            .map(|attempt| attempt.split(':').next().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["1", "3", "4", "2"]
    );
    assert_eq!(result.pages[0].text, "kept");
    assert_eq!(result.pages[1].text, "deferred");
    assert_eq!(result.pages[2].status, Status::Skipped);
    assert_eq!(result.pages[2].text, "");
    assert!(result.pages[2].reason.is_some());
    assert_eq!(result.pages[3].text, "healthy neighbour");
}

#[test]
fn malformed_singleton_protocol_is_fatal() {
    let worker = Worker::new(&format!(
        "if [ \"$9\" != \"${{10}}\" ]; then\n{}\nexec /bin/sleep 30\nfi\n\
         {}\nprintf 'not-json\\n'",
        response(3, vec![page(1, Status::Text, "kept")], false),
        response(3, vec![], false),
    ));
    let result = worker.run(&options(3), &Limits::default(), Duration::from_secs(5));
    assert!(matches!(result, Err(Error::Malformed { .. })), "{result:?}");
    assert_eq!(worker.attempts().len(), 2, "fatal protocol error must stop scheduling");
}

#[test]
fn singleton_spawn_io_failure_is_fatal() {
    // Removing execute permission does not interrupt the running shell, but
    // the next spawn fails with EACCES (the path remains a valid file).
    let worker = Worker::new(&format!(
        "{}\nchmod 600 \"$0\" || exit 1\nexec /bin/sleep 30",
        response(3, vec![page(1, Status::Text, "kept")], false),
    ));
    let result = worker.run(&options(3), &Limits::default(), Duration::from_secs(5));
    assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
    assert_eq!(worker.attempts().len(), 1);
}
