use std::assert_matches;

use super::*;
use crate::doc::{Document as Doc, Paragraph, Run, Table};
use crate::read::{Limits, markdown as read};
use crate::test_support::{page_pdf, pdf_of, stream_object};

const IMAGE: &str = "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                     /ColorSpace /DeviceGray /BitsPerComponent 8";

fn text(pdf: &[u8]) -> String {
    read(pdf, &Limits::default()).unwrap()
}

#[test]
fn reads_what_the_writer_wrote() {
    let mut doc = Doc::new();
    doc.set_page_numbers(true);
    doc.heading(1, "Quarterly report").unwrap();
    doc.heading(2, "Sales").unwrap();
    doc.push_paragraph(
        Paragraph::new().text("Revenue was ").run(Run::new("up 12%").bold()).text(" this quarter."),
    )
    .unwrap();
    doc.table(Table::new([["Region", "Q1"], ["North", "12.5"]]).header()).unwrap();
    for _ in 0..3 {
        doc.page_break();
        doc.paragraph("Appendix page.").unwrap();
    }
    let text = text(&doc.to_pdf().unwrap());
    assert!(
        text.starts_with("# Quarterly report\n\n## Sales\n\nRevenue was up 12% this quarter.\n"),
        "{text}"
    );
    assert!(text.contains("Region Q1") && text.contains("North 12.5"), "{text}");
    assert!(text.contains("<!-- page 4 -->\n\nAppendix page.\n"), "{text}");
    assert!(!text.contains("1 / 4"), "page numbers repeat in the footer: {text}");
}

#[test]
fn spaces_words_from_kerning_and_positions() {
    let content = "BT /F1 12 Tf 72 700 Td [(Hel) -20 (lo) -300 (World)] TJ \
                   0 -14 Td (second) Tj 60 0 Td (line) Tj ET";
    assert_eq!(text(&page_pdf(content, "")), "Hello World\nsecond line\n");
}

#[test]
fn reads_rotated_pages_upright() {
    // On a page turned a quarter, text drawn turned back reads upright.
    let content = "BT /F1 12 Tf 0 1 -1 0 300 100 Tm (Turned) Tj ET";
    assert_eq!(text(&page_pdf(content, "/Rotate 90")), "Turned\n");
}

#[test]
fn reads_text_inside_forms_once() {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
          /Resources << /Font << /F1 4 0 R >> /XObject << /X 6 0 R /Image 7 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream_object("", b"/X Do /Image Do q 1 0 0 1 0 -100 cm /X Do Q"),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792] \
             /Resources << /Font << /F1 4 0 R >> /XObject << /Self 6 0 R >> >>",
            b"BT /F1 12 Tf 72 700 Td (In a form) Tj ET /Self Do",
        ),
        stream_object(IMAGE, b"\x00"),
    ];
    assert_eq!(
        text(&pdf_of(&objects)),
        "In a form\n\nIn a form\n",
        "drawn twice, 100pt apart; the self-reference is not followed"
    );
}

#[test]
fn scanned_pages_have_no_text_layer() {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
          /Resources << /XObject << /Scan 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        stream_object("", b"q 612 0 0 792 0 0 cm /Scan Do Q BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI"),
        stream_object(IMAGE, b"\x00"),
    ];
    assert_matches!(read(&pdf_of(&objects), &Limits::default()), Err(Error::NoTextLayer));
}

#[test]
fn rebuilds_a_broken_cross_reference_table() {
    let mut pdf = page_pdf("BT /F1 12 Tf 72 700 Td (Still here) Tj ET", "");
    let xref = pdf.windows(4).rposition(|window| window == b"xref").unwrap();
    pdf.truncate(xref);
    assert_eq!(
        text(&pdf),
        "Still here\n",
        "no table, no trailer: the catalog is found by scanning"
    );
}

#[test]
fn drops_text_outside_the_page_and_running_lines() {
    let page = |number: usize| {
        format!(
            "BT /F1 10 Tf 72 30 Td (Page {number} of 3) Tj ET \
             BT /F1 12 Tf 72 700 Td (Body {number}) Tj ET BT /F1 12 Tf -500 700 Td (hidden) Tj ET"
        )
    };
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] \
          /Resources << /Font << /F1 9 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>".to_vec(),
        stream_object("", page(1).as_bytes()),
        stream_object("", page(2).as_bytes()),
        stream_object("", page(3).as_bytes()),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    assert_eq!(
        text(&pdf_of(&objects)),
        "Body 1\n\n<!-- page 2 -->\n\nBody 2\n\n<!-- page 3 -->\n\nBody 3\n",
        "inherited resources and media box"
    );
}

#[test]
fn opens_encrypted_documents_with_an_empty_password() {
    const CASES: &[(&str, &[u8], &str)] = &[
        (
            "RC4 40-bit, CoreGraphics",
            include_bytes!("tests/rc4-40-coregraphics.pdf"),
            "Secret certificate 40",
        ),
        (
            "AES-128, CoreGraphics",
            include_bytes!("tests/aes-128-coregraphics.pdf"),
            "Secret certificate 128",
        ),
        ("RC4 128-bit, pypdf", include_bytes!("tests/rc4-128-pypdf.pdf"), "# A Simple PDF File"),
        (
            "AES-256 revision 5, pypdf",
            include_bytes!("tests/aes-256-r5-pypdf.pdf"),
            "# A Simple PDF File",
        ),
        (
            "AES-256 revision 6, pypdf",
            include_bytes!("tests/aes-256-r6-pypdf.pdf"),
            "# A Simple PDF File",
        ),
    ];
    for &(label, pdf, expected) in CASES {
        assert!(text(pdf).starts_with(expected), "{label}: {}", text(pdf));
    }
    assert_matches!(
        read(include_bytes!("tests/password-pypdf.pdf"), &Limits::default()),
        Err(Error::PasswordRequired)
    );
}

#[test]
fn limits_stop_a_read() {
    let pdf = page_pdf("BT /F1 12 Tf 72 700 Td (x) Tj ET", "");
    assert_matches!(
        read(&pdf, &Limits::default().max_input(10)),
        Err(Error::LimitExceeded("input size"))
    );
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert_matches!(read(&pdf, &Limits::default().cancel_on(flag)), Err(Error::Cancelled));
}
