use std::fmt::Write as _;
use std::fs;
use std::iter;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::protocol;
use super::stream::{self, Frame};
use crate::read::pdf::{Info, PageResult, Status};

// MOCKED: a shell script replaying canned worker frames in place of the PDF worker.
pub(super) struct Worker {
    pub directory: tempfile::TempDir,
    pub path: PathBuf,
}

impl Worker {
    pub fn new(body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("worker");
        // $9 is first_page, ${10} last_page, ${13} max_output_bytes.
        let script = format!(
            "#!/bin/sh\n[ \"$#\" -eq 0 ] && exit 0\ncd \"$(dirname \"$0\")\" || exit 1\n\
             printf '%s:%s:%s\\n' \"$9\" \"${{10}}\" \"${{13}}\" >> attempts\n{body}\n"
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(Command::new(&path).status().unwrap().success(), "fake worker did not start");
        Self { directory, path }
    }

    pub fn recovering(batch: &str, singles: &[(&str, String)]) -> Self {
        let mut body = format!(
            "if [ \"$9\" != \"${{10}}\" ]; then\n{batch}\nexec /bin/sleep 30\nfi\ncase \"$9\" in\n"
        );
        for (pattern, reply) in singles {
            writeln!(body, "{pattern}) {reply};;").unwrap();
        }
        body.push_str("*) exit 99;;\nesac");
        Self::new(&body)
    }

    pub fn attempts(&self) -> Vec<String> {
        match fs::read_to_string(self.directory.path().join("attempts")) {
            Ok(log) => log.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read worker attempt log: {error}"),
        }
    }
}

pub(super) fn page(number: u32, status: Status, text: &str) -> PageResult {
    PageResult {
        number,
        status,
        text: text.into(),
        images: matches!(status, Status::Text | Status::Empty).then_some(0),
        truncated: false,
        reason: matches!(status, Status::Failed | Status::Skipped).then(|| "page-local".into()),
    }
}

pub(super) fn reply(count: u32, pages: Vec<PageResult>, last: Option<Frame>) -> String {
    let info = Frame::Info(Info { page_count: count, encrypted: false });
    let mut bytes = protocol::HEADER.to_vec();
    for frame in iter::once(info).chain(pages.into_iter().map(Frame::Page)).chain(last) {
        stream::write(&mut bytes, &frame).unwrap();
    }
    let mut escaped = String::new();
    for byte in bytes {
        write!(&mut escaped, "\\{byte:03o}").unwrap();
    }
    format!("printf '{escaped}'")
}
