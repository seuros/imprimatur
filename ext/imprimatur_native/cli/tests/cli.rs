//! The binary itself, on documents written with the imprimatur crate.

// The helpers below are test code too, but outside a `#[test]` function, so
// clippy's allow-unwrap-in-tests does not cover them.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use imprimatur::doc::Document;
use imprimatur::xlsx::{Cell, Comment, Workbook};
use serde_json::{Value, json};
use tempfile::TempDir;
use usage_rs::test::command;

fn workbook() -> Vec<u8> {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Sales").unwrap();
    sheet.append_row(["Region", "Q1"]).unwrap().append_row(["North", "12.5"]).unwrap();
    book.to_bytes().unwrap()
}

fn pdf(text: &str) -> Vec<u8> {
    let mut doc = Document::new();
    doc.paragraph(text).unwrap();
    doc.to_pdf().unwrap()
}

/// A sheet of typed cells, one with a comment.
fn ledger() -> Vec<u8> {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Ledger").unwrap();
    sheet.append_row(["Item", "Amount", "Paid"]).unwrap();
    sheet.append_row([Cell::from("Audit fee"), Cell::from(1250.5), Cell::from(true)]).unwrap();
    sheet
        .set_comment(1, 1, Comment::new("Invoice INV-7, net of VAT").unwrap().author("Finance"))
        .unwrap();
    book.to_bytes().unwrap()
}

/// Two sections of two paragraphs.
fn report() -> Vec<u8> {
    let mut doc = Document::new();
    doc.heading(1, "Scope").unwrap();
    doc.paragraph("Covers organic cotton.").unwrap();
    doc.paragraph("Excludes dyeing.").unwrap();
    doc.heading(1, "Findings").unwrap();
    doc.paragraph("One minor nonconformity.").unwrap();
    doc.paragraph("Closed in March.").unwrap();
    doc.to_docx().unwrap()
}

/// A page drawing a 2 by 2 gray image at (72, 600), 100 by 50 points, and
/// a 1 by 1 one in the corner.
fn figures() -> Vec<u8> {
    let stream = |dict: &str, data: &[u8]| {
        let mut object = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        object.extend_from_slice(data);
        object.extend_from_slice(b"\nendstream");
        object
    };
    let gray = "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /BitsPerComponent 8";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
          /Resources << /XObject << /A 5 0 R /B 6 0 R >> >> >>"
            .to_vec(),
        stream("", b"q 100 0 0 50 72 600 cm /A Do Q /B Do"),
        stream(&format!("{gray} /Width 2 /Height 2"), &[0, 85, 170, 255]),
        stream(&format!("{gray} /Width 1 /Height 1"), &[128]),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (number, object) in (1..).zip(&objects) {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let (xref, size) = (pdf.len(), objects.len() + 1);
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    let trailer = format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    pdf.extend_from_slice(trailer.as_bytes());
    pdf
}

/// A workbook, a PDF and a Word document: `ledger.xlsx`, `memo.pdf` and
/// `report.docx`.
fn documents() -> TempDir {
    files(&[
        ("ledger.xlsx", &ledger()),
        ("memo.pdf", &pdf("Hello there.")),
        ("report.docx", &report()),
    ])
}

/// The binary run with `args`.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_imprimatur")).args(args).output().unwrap()
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

fn json_lines(text: &str) -> Vec<Value> {
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// `documents` written into a new directory, by name.
fn files(documents: &[(&str, &[u8])]) -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    for (name, bytes) in documents {
        std::fs::write(directory.path().join(name), bytes).unwrap();
    }
    directory
}

fn path(directory: &TempDir, name: &str) -> String {
    directory.path().join(name).to_str().unwrap().to_owned()
}

const TABLE: &str = "## Sales\n\n| Region | Q1 |\n| --- | --- |\n| North | 12.5 |\n";

#[test]
fn prints_documents() {
    let dir = files(&[("sales.xlsx", &workbook()), ("memo.pdf", &pdf("Hello there."))]);
    let one = command!("imprimatur", "markdown", &path(&dir, "sales.xlsx")).assert_success();
    assert_eq!(one.stdout_text(), TABLE);

    let both = command!("imprimatur", "md", &path(&dir, "sales.xlsx"), &path(&dir, "memo.pdf"))
        .assert_success();
    let expected = format!(
        "<!-- file: {} -->\n\n{TABLE}<!-- file: {} -->\n\nHello there.\n",
        path(&dir, "sales.xlsx"),
        path(&dir, "memo.pdf")
    );
    assert_eq!(both.stdout_text(), expected);
}

#[test]
fn writes_files_with_distinct_names() {
    let dir = files(&[("sales.xlsx", &workbook())]);
    std::fs::create_dir(dir.path().join("again")).unwrap();
    std::fs::write(dir.path().join("again/sales.xlsx"), workbook()).unwrap();
    let out = path(&dir, "out");
    command!(
        "imprimatur",
        "md",
        "-o",
        &out,
        &path(&dir, "sales.xlsx"),
        &path(&dir, "again/sales.xlsx")
    )
    .assert_success();
    for name in ["sales.md", "sales-2.md"] {
        assert_eq!(std::fs::read_to_string(Path::new(&out).join(name)).unwrap(), TABLE, "{name}");
    }
}

#[test]
fn reports_failures_with_their_exit_status() {
    const CASES: &[(&str, i32, &str)] = &[
        ("scan.pdf", 3, "no text layer"),
        ("notes.txt", 5, "cannot read this document"),
        ("missing.pdf", 1, "No such file"),
    ];
    let dir = files(&[
        ("scan.pdf", &pdf(" ")),
        ("notes.txt", b"just text"),
        ("broken.pdf", b"%PDF-1.7\nnothing else"),
        ("sales.xlsx", &workbook()),
    ]);
    for &(name, code, message) in CASES {
        let output = command!("imprimatur", "md", &path(&dir, name));
        assert_eq!(output.status.code(), Some(code), "{name}: {}", output.stderr_text());
        assert!(output.stderr_text().contains(message), "{name}: {}", output.stderr_text());
        assert_eq!(output.stdout_text(), "", "{name}");
    }

    // The others are still read; the status is the first failure's.
    let output = command!(
        "imprimatur",
        "md",
        &path(&dir, "notes.txt"),
        &path(&dir, "sales.xlsx"),
        &path(&dir, "scan.pdf")
    );
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout_text().contains(TABLE), "{}", output.stdout_text());

    let limited = command!("imprimatur", "md", "--max-bytes", "10", &path(&dir, "sales.xlsx"));
    assert_eq!(limited.status.code(), Some(6), "{}", limited.stderr_text());
    let wrong = command!("imprimatur", "md", "--timeout", "-1", &path(&dir, "sales.xlsx"));
    assert_eq!(wrong.status.code(), Some(2), "{}", wrong.stderr_text());
}

#[test]
fn reports_json_lines() {
    let dir = files(&[("sales.xlsx", &workbook()), ("scan.pdf", &pdf(" "))]);
    let output =
        command!("imprimatur", "--json", "md", &path(&dir, "sales.xlsx"), &path(&dir, "scan.pdf"));
    assert_eq!(output.status.code(), Some(3));
    let lines = json_lines(output.stdout_text());
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["ok"], true);
    assert_eq!(
        (&lines[0]["format"], &lines[0]["sheets"][0]["name"]),
        (&json!("xlsx"), &json!("Sales"))
    );
    assert_eq!(lines[0]["file"], path(&dir, "sales.xlsx"));
    assert!(lines[0]["milliseconds"].is_u64());
    assert_eq!(lines[1]["ok"], false);
    assert_eq!(lines[1]["error"], "no_text_layer");
    assert_eq!(output.stderr_text(), "", "--json reports failures on stdout only");
}

#[test]
fn reports_structure_as_json() {
    let dir = documents();
    let files = [path(&dir, "ledger.xlsx"), path(&dir, "memo.pdf"), path(&dir, "report.docx")];
    let output = run(&["--json", "md", &files[0], &files[1], &files[2]]);
    assert!(output.status.success(), "{output:?}");
    let lines = json_lines(stdout(&output));

    let sheet = &lines[0]["sheets"][0];
    let row = json!({ "row": 2, "cells": [
        { "cell": "A2", "type": "text", "value": "Audit fee" },
        { "cell": "B2", "type": "number", "value": 1250.5, "text": "1250.5" },
        { "cell": "C2", "type": "bool", "value": true, "text": "TRUE" },
    ] });
    assert_eq!(sheet["rows"][1], row);
    let note = json!([{ "cell": "B2", "author": "Finance", "text": "Invoice INV-7, net of VAT" }]);
    assert_eq!(sheet["comments"], note);

    let page = &lines[1]["pages"][0];
    assert_eq!((&page["page"], &page["images"]), (&json!(1), &json!(0)));
    let block = &page["blocks"][0];
    assert_eq!((&block["type"], &block["text"]), (&json!("paragraph"), &json!("Hello there.")));
    let bounds = &block["bounds"];
    assert!(bounds["left"].as_f64() < bounds["right"].as_f64(), "{bounds}");
    assert!(bounds["top"].as_f64() < bounds["bottom"].as_f64(), "{bounds}");

    let blocks = &lines[2]["blocks"];
    assert_eq!(blocks[0], json!({ "type": "heading", "level": 1, "text": "Scope" }));
    assert_eq!(blocks[1], json!({ "type": "paragraph", "text": "Covers organic cotton." }));
}

#[test]
fn greps_documents_by_location() {
    let dir = documents();
    let (ledger, memo, report) =
        (path(&dir, "ledger.xlsx"), path(&dir, "memo.pdf"), path(&dir, "report.docx"));
    let context = "§Scope- Scope\n§Scope: Covers organic cotton.\n§Scope- Excludes dyeing.\n--\n\
                   §Findings- One minor nonconformity.\n§Findings: Closed in March.\n";
    let cases: &[(&[&str], String, i32)] = &[
        (&["-i", "audit", &ledger], "Ledger!A2: Audit fee\n".into(), 0),
        (&["VAT", &ledger], "Ledger!B2 (comment): Invoice INV-7, net of VAT\n".into(), 0),
        (&["-w", "there", &memo], "p1: Hello there.\n".into(), 0),
        (&["-w", "her", &memo], String::new(), 1),
        (&["-F", "1250.5", &ledger, &memo], format!("{ledger}:Ledger!B2: 1250.5\n"), 0),
        (&["-c", "e", &ledger, &memo], format!("{ledger}:3\n{memo}:1\n"), 0),
        (&["-l", "Hello", &ledger, &memo], format!("{memo}\n"), 0),
        (&["-q", "Hello", &memo], String::new(), 0),
        (&["-m", "1", "e", &report], "§Scope: Scope\n".into(), 0),
        (&["-C", "1", "Covers|Closed", &report], context.into(), 0),
    ];
    for (args, expected, code) in cases {
        let output = run(&[&["grep"], *args].concat());
        assert_eq!(stdout(&output), expected, "{args:?}");
        assert_eq!(output.status.code(), Some(*code), "{args:?}: {output:?}");
    }

    let output = run(&["--json", "grep", "-i", "FEE", &ledger]);
    let found = json!({
        "file": ledger, "type": "match", "location": { "sheet": "Ledger", "cell": "A2" },
        "line": 4, "text": "Audit fee", "matches": [{ "start": 6, "end": 9 }],
    });
    assert_eq!(json_lines(stdout(&output)), [found]);
}

#[test]
fn grep_exits_two_on_trouble() {
    let dir = files(&[("ledger.xlsx", &ledger()), ("scan.pdf", &pdf(" "))]);
    let (ledger, scan) = (path(&dir, "ledger.xlsx"), path(&dir, "scan.pdf"));
    let output = run(&["grep", "Audit", &ledger, &scan]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!("{ledger}:Ledger!A2: Audit fee\n"),
        "the others are searched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("no text layer"), "{output:?}");

    assert_eq!(
        run(&["grep", "-q", "Audit", &scan, &ledger]).status.code(),
        Some(0),
        "a match wins with -q"
    );
    let output = run(&["--json", "grep", "Audit", &scan]);
    assert_eq!(json_lines(stdout(&output))[0]["error"], "no_text_layer");
    let output = run(&["grep", "(", &ledger]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid pattern"), "{output:?}");
}

#[test]
fn extracts_images() {
    let dir = files(&[("figures.pdf", &figures()), ("memo.pdf", &pdf("No pictures."))]);
    let (figures, memo, out) =
        (path(&dir, "figures.pdf"), path(&dir, "memo.pdf"), path(&dir, "out"));
    let output = run(&["images", "-o", &out, &figures, &memo, &figures]);
    assert!(output.status.success(), "{output:?}");
    let names =
        ["figures-p1-1.png", "figures-p1-2.png", "figures-p1-1-2.png", "figures-p1-2-2.png"];
    let listed = names.map(|name| format!("{out}/{name}\n")).concat();
    assert_eq!(stdout(&output), listed, "a document read twice keeps both");
    for name in names {
        let image = std::fs::read(Path::new(&out).join(name)).unwrap();
        assert!(image.starts_with(b"\x89PNG"), "{name}");
    }

    let output =
        run(&["--json", "images", "-o", &path(&dir, "large"), "--min-size", "2", &figures]);
    let written = path(&dir, "large/figures-p1-1.png");
    let image = json!({
        "file": figures, "image": written, "format": "png", "width": 2, "height": 2,
        "bytes": std::fs::metadata(&written).unwrap().len(), "page": 1,
        "bounds": { "left": 72.0, "top": 142.0, "right": 172.0, "bottom": 192.0 },
    });
    assert_eq!(json_lines(stdout(&output)), [image], "the 1 by 1 image is smaller than 2");
}

#[test]
fn reads_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_imprimatur"))
        .args(["md", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&workbook()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), TABLE);
}

#[test]
fn prints_completions_and_its_spec() {
    let zsh = command!("imprimatur", "completion", "zsh").assert_success();
    assert!(zsh.stdout_text().contains("imprimatur"), "{}", zsh.stdout_text());
    let spec = command!("imprimatur", "__usage_spec__").assert_success();
    assert!(spec.stdout_text().contains("\nbin imprimatur\n"), "{}", spec.stdout_text());
}
