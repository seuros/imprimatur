use super::*;
use crate::doc::PageSize;

#[test]
fn wraps_body_text_at_the_document_font_size() {
    let mut doc = Document::new();
    doc.set_page_size(PageSize::custom(100.0, 100.0).unwrap()).set_margin(10.0).unwrap();
    for (points, lines) in [(10.5, 1), (20.0, 2)] {
        doc.set_font_size(points).unwrap();
        let mut layout = Layout::new(&doc).unwrap();
        layout.paragraph(&Paragraph::new().text("aaaa aaaa")).unwrap();
        let pages = layout.finish(false);
        assert_eq!(pages.len(), 1);
        let page = String::from_utf8_lossy(&pages[0]);
        assert!(page.contains(&format!("/F1 {points} Tf")), "{page}");
        assert_eq!(page.matches(" Tj ET").count(), lines, "{page}");
        if lines == 1 {
            assert!(page.contains("(aaaa aaaa) Tj"), "{page}");
        } else {
            assert_eq!(page.matches("(aaaa) Tj").count(), 2, "{page}");
        }
    }
}

#[test]
fn paginates_body_lines_at_the_document_font_size() {
    let mut doc = Document::new();
    doc.set_page_size(PageSize::custom(100.0, 100.0).unwrap()).set_margin(10.0).unwrap();
    for (points, counts) in [(10.0, vec![4]), (20.0, vec![3, 1])] {
        doc.set_font_size(points).unwrap();
        let mut layout = Layout::new(&doc).unwrap();
        layout.paragraph(&Paragraph::new().text("one\ntwo\nthree\nfour")).unwrap();
        let pages = layout.finish(false);
        let lines: Vec<_> = pages
            .iter()
            .map(|page| String::from_utf8_lossy(page).matches(" Tj ET").count())
            .collect();
        assert_eq!(lines, counts, "{points}pt");
        assert!(String::from_utf8_lossy(pages.last().unwrap()).contains("(four) Tj"));
    }
}

#[test]
fn reserves_the_document_font_size_after_headings() {
    let mut doc = Document::new();
    doc.set_page_size(PageSize::custom(200.0, 200.0).unwrap()).set_margin(10.0).unwrap();
    for (points, heading_page) in [(10.0, 0), (30.0, 1)] {
        doc.set_font_size(points).unwrap();
        let mut layout = Layout::new(&doc).unwrap();
        // The table leaves room for a heading and two 10pt lines, but not 30pt lines.
        let table = Table::new([["filler\nfiller\nfiller\nfiller\nfiller\nfiller\nfiller"]]);
        layout.table(&table).unwrap();
        layout.heading(2, "Section").unwrap();
        layout.paragraph(&Paragraph::new().text("first\nsecond")).unwrap();
        let pages = layout.finish(false);
        assert_eq!(pages.len(), heading_page + 1);
        let page = String::from_utf8_lossy(&pages[heading_page]);
        for text in ["Section", "first", "second"] {
            assert!(page.contains(&format!("({text}) Tj")), "{points}pt: {page}");
        }
    }
}

#[test]
fn keeps_a_heading_with_what_follows() {
    let mut layout = Layout::new(&Document::new()).unwrap();
    // Fill the page to just above the bottom margin.
    let near_bottom = layout.margin + 30.0;
    for _ in 0..100 {
        if layout.y <= near_bottom {
            break;
        }
        layout.paragraph(&Paragraph::new().text("filler")).unwrap();
    }
    assert!(layout.y <= near_bottom && layout.pages.len() == 1, "filled the first page");
    layout.heading(2, "Section").unwrap();
    let pages = layout.finish(false);
    assert_eq!(pages.len(), 2);
    assert!(String::from_utf8_lossy(&pages[1]).contains("(Section) Tj"));
}

#[test]
fn numbers_are_short() {
    for (value, text) in [
        (1.0, "1"),
        (0.5, "0.5"),
        (12.345, "12.35"),
        (-0.001, "0"),
        (595.276, "595.28"),
        (-2.5, "-2.5"),
        (0.3, "0.3"),
        (10.0, "10"),
    ] {
        assert_eq!(Num(value).to_string(), text, "{value}");
    }
}
