use std::assert_matches;

use super::*;
use crate::doc::Document as Writer;
use crate::test_support::{pdf_of, stream_object};

fn damaged() -> Vec<u8> {
    pdf_of(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 100000 \
          /MediaBox [0 0 612 792] /Resources << /Font << /F1 11 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 10 0 R >>".to_vec(),
        stream_object("", b"BT /F1 12 Tf 72 700 Td (First) Tj ET"),
        stream_object("/Filter /FlateDecode", b"broken compressed data"),
        b"null".to_vec(),
        stream_object("", b"BT /F1 12 Tf 72 700 Td (Last) Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

#[test]
fn counts_without_decoding_and_reads_only_requested_pages() {
    let pdf = damaged();
    let limits = Limits::default().max_inflated(0);
    assert_eq!(info(&pdf, &limits).unwrap().page_count, 4);
    let options = ReadOptions { first_page: 4, last_page: 4, ..ReadOptions::default() };
    let result = pages(&pdf, &options, &Limits::default()).unwrap();
    assert_eq!(result.info.page_count, 4);
    assert_eq!(result.pages.len(), 1);
    assert_eq!((result.pages[0].number, result.pages[0].text.as_str()), (4, "Last"));
    let options = ReadOptions { max_pages: 1, ..ReadOptions::default() };
    let result = pages(&pdf, &options, &Limits::default()).unwrap();
    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.info.page_count, 4);
}

#[test]
fn a_damaged_page_is_neither_blank_nor_the_loss_of_healthy_pages() {
    for (status, expected) in [
        (Status::Text, "text"),
        (Status::Empty, "empty"),
        (Status::Failed, "failed"),
        (Status::Skipped, "skipped"),
    ] {
        assert_eq!(status.as_str(), expected);
    }
    let result = pages(&damaged(), &ReadOptions::default(), &Limits::default()).unwrap();
    assert_eq!(
        result.pages.iter().map(|page| page.status).collect::<Vec<_>>(),
        [Status::Text, Status::Failed, Status::Empty, Status::Text]
    );
    assert_eq!(result.pages[1].images, None);
    assert!(result.pages[1].reason.as_ref().is_some_and(|reason| reason.contains("Flate")));
    assert_eq!(result.pages[3].text, "Last");
}

#[test]
fn truncation_is_utf8_safe_and_output_caps_stop_later_pages() {
    let mut doc = Writer::new();
    doc.paragraph("éééé").unwrap();
    doc.page_break();
    doc.paragraph("Tail").unwrap();
    doc.page_break();
    doc.paragraph("Last").unwrap();
    let data = doc.to_pdf().unwrap();
    let options = ReadOptions { max_page_bytes: 5, max_output_bytes: 5, ..ReadOptions::default() };
    let result = pages(&data, &options, &Limits::default()).unwrap();
    assert_eq!(result.pages[0].text, "éé");
    assert!(result.pages[0].truncated);
    assert_eq!(result.pages[1].text, "T");
    assert_eq!(result.pages[2].status, Status::Skipped);
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    assert_matches!(
        pages(&data, &options, &Limits::default().cancel_on(cancelled)),
        Err(Error::Cancelled)
    );
}
