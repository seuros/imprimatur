use std::assert_matches;

use super::*;
use crate::style::{Align, TextDirection};

#[test]
fn builds_blocks_in_order() {
    let mut doc = Document::new();
    doc.heading(1, "Title").unwrap().paragraph("Body").unwrap().page_break();
    doc.table(Table::new([vec!["a", "b"], vec!["c"]]).header()).unwrap();
    assert_matches!(
        doc.blocks(),
        [Block::Heading { level: 1, .. }, Block::Paragraph(_), Block::PageBreak, Block::Table(_)]
    );
    let Some(Block::Table(table)) = doc.blocks().last() else { unreachable!() };
    assert_eq!(table.columns(), 2, "short rows are padded");
}

#[test]
fn validates_content() {
    let mut doc = Document::new();
    assert_matches!(doc.heading(0, "x"), Err(Error::InvalidHeadingLevel(0)));
    assert_matches!(doc.heading(4, "x"), Err(Error::InvalidHeadingLevel(4)));
    assert_matches!(doc.paragraph("bell\u{7}"), Err(Error::ControlCharacter(7)));
    assert_matches!(doc.table(Table::new(Vec::<Vec<String>>::new())), Err(Error::EmptyTable));
    assert_matches!(doc.table(Table::new([Vec::<String>::new()])), Err(Error::EmptyTable));
    assert_matches!(doc.table(Table::new([["\u{1b}"]])), Err(Error::ControlCharacter(0x1B)));
    assert!(doc.paragraph("tab\there\nnew line").is_ok());
    assert_eq!(doc.blocks().len(), 1, "rejected content is not added");
}

#[test]
fn right_to_left_paragraphs_align_right_by_default() {
    let rtl = Paragraph::new().text("שלום").direction(TextDirection::RightToLeft);
    assert_eq!(rtl.effective_align(), Align::Right);
    assert_eq!(rtl.align(Align::Center).effective_align(), Align::Center);
    assert_eq!(Paragraph::new().effective_align(), Align::Left);
}
