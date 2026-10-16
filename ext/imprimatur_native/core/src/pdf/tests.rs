use std::assert_matches;

use crate::Error;
use crate::doc::{Document, Paragraph, Run, Table};
use crate::style::{Align, Color};

/// Checks the file's structure and returns each page's decompressed content.
fn pages(pdf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf);
    assert!(pdf.starts_with(b"%PDF-1.7\n"));
    assert!(text.ends_with("%%EOF\n"));
    let startxref: usize =
        text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(pdf[startxref..].starts_with(b"xref\n"), "startxref points at the table");
    // Byte offsets: the lossy text is longer than the bytes (binary header).
    let table = String::from_utf8_lossy(&pdf[startxref..]);
    let size: usize = table.lines().nth(1).unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    for (id, entry) in table.lines().skip(3).take(size - 1).enumerate() {
        let offset: usize = entry[..10].parse().unwrap();
        let expected = format!("{} 0 obj\n", id + 1);
        assert!(pdf[offset..].starts_with(expected.as_bytes()), "object {} offset", id + 1);
    }
    let mut contents = Vec::new();
    let mut rest = pdf;
    while let Some(start) = find(rest, b"stream\n") {
        let body = &rest[start + 7..];
        let end = find(body, b"\nendstream").unwrap();
        let data = miniz_oxide::inflate::decompress_to_vec_zlib(&body[..end]).unwrap();
        contents.push(String::from_utf8_lossy(&data).into_owned());
        rest = &body[end + b"\nendstream".len()..];
    }
    let count: usize =
        text.split("/Count ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert_eq!(contents.len(), count, "one content stream per page");
    contents
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[test]
fn writes_a_valid_single_page() {
    let mut doc = Document::new();
    doc.heading(1, "Report (draft)").unwrap();
    doc.push_paragraph(
        Paragraph::new().text("Plain, ").run(Run::new("bold").bold().color(Color::rgb(0xFF_00_00))),
    )
    .unwrap();
    let pdf = doc.to_pdf().unwrap();
    let pages = pages(&pdf);
    assert_eq!(pages.len(), 1);
    assert!(pages[0].contains(r"/F2 20 Tf"), "heading in bold 20pt: {}", pages[0]);
    assert!(pages[0].contains(r"(Report \(draft\)) Tj"), "parentheses escaped: {}", pages[0]);
    assert!(pages[0].contains("1 0 0 rg"), "red run: {}", pages[0]);
    assert!(
        String::from_utf8_lossy(&pdf)
            .contains("/BaseFont /Helvetica-BoldOblique /Encoding /WinAnsiEncoding")
    );
    assert_eq!(doc.to_pdf().unwrap(), pdf, "deterministic");
}

#[test]
fn paginates_and_numbers_pages() {
    let mut doc = Document::new();
    doc.set_page_numbers(true);
    for index in 0..120 {
        doc.paragraph(format!("Paragraph {index}: the quick brown fox jumps over the lazy dog."))
            .unwrap();
    }
    doc.page_break().paragraph("After the break.").unwrap();
    let pages = pages(&doc.to_pdf().unwrap());
    assert!(pages.len() >= 4, "{} pages", pages.len());
    let total = pages.len();
    for (index, page) in pages.iter().enumerate() {
        assert!(page.contains(&format!("({} / {total}) Tj", index + 1)), "page {index}");
    }
    assert!(pages.last().unwrap().contains("(After the break.) Tj"));
    assert!(!pages[total - 2].contains("After the break"));
}

#[test]
fn aligns_lines() {
    let mut doc = Document::new();
    doc.push_paragraph(Paragraph::new().text("x").align(Align::Right)).unwrap();
    let page = &pages(&doc.to_pdf().unwrap())[0];
    // A4 is 595.28pt wide; one-inch margins; "x" is 5.5pt at 11pt.
    assert!(page.contains("517.78 "), "right aligned at 595.28 - 72 - 5.5: {page}");
}

#[test]
fn repeats_table_headers_across_pages() {
    let mut rows = vec![vec!["Region".to_owned(), "Amount".to_owned()]];
    rows.extend((0..80).map(|index| vec![format!("Region {index}"), format!("{index}.00")]));
    let mut doc = Document::new();
    doc.table(Table::new(rows).header()).unwrap();
    let pages = pages(&doc.to_pdf().unwrap());
    assert!(pages.len() >= 2);
    for page in &pages {
        assert!(
            page.contains("/F2 10 Tf 0 0 0 rg") && page.contains("(Region) Tj"),
            "header on every page"
        );
        assert!(page.contains("0.85 0.89 0.95 rg"), "header shading");
    }
}

#[test]
fn refuses_what_it_cannot_draw() {
    let mut doc = Document::new();
    doc.paragraph("مرحبا").unwrap();
    assert_matches!(
        doc.to_pdf(),
        Err(Error::UnsupportedCharacter { character: 'م', font: "Helvetica" })
    );
    assert!(doc.to_docx().is_ok(), "DOCX carries any script");

    let mut tall = Document::new();
    tall.table(Table::new([["line\n".repeat(80)]])).unwrap();
    assert_matches!(tall.to_pdf(), Err(Error::RowTooTall(_)));
}

#[test]
fn an_empty_document_is_one_blank_page() {
    assert_eq!(pages(&Document::new().to_pdf().unwrap()), [""]);
}
