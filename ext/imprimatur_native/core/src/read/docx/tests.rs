use crate::doc::{Document, Paragraph, Run, Table as DocTable};
use crate::read::{
    Block, Content, ImageFormat, ImageLocation, Limits, Mark, content, images, markdown as read,
};
use crate::test_support::zip_of;

#[test]
fn reads_what_the_writer_wrote() {
    let mut doc = Document::new();
    doc.heading(1, "Quarterly report").unwrap();
    doc.heading(2, "Sales").unwrap();
    doc.push_paragraph(
        Paragraph::new().text("Revenue was ").run(Run::new("up 12%").bold()).text(" this quarter."),
    )
    .unwrap();
    doc.table(DocTable::new([["Region", "Q1"], ["North", "12.5"]]).header()).unwrap();
    let text = read(&doc.to_docx().unwrap(), &Limits::default()).unwrap();
    assert_eq!(
        text,
        "# Quarterly report\n\n## Sales\n\nRevenue was **up 12%** this quarter.\n\n\
         | Region | Q1 |\n| --- | --- |\n| North | 12.5 |\n"
    );
}

/// A package of the parts in `tests/` and `extra` parts, read with our own
/// writer's zip.
fn package(extra: &[(&str, &str)]) -> Vec<u8> {
    let mut parts = vec![
        ("[Content_Types].xml", include_str!("tests/content_types.xml")),
        ("_rels/.rels", include_str!("tests/package.rels")),
        ("word/document.xml", include_str!("tests/document.xml")),
        ("word/_rels/document.xml.rels", include_str!("tests/document.xml.rels")),
        ("word/styles.xml", include_str!("tests/styles.xml")),
        ("word/numbering.xml", include_str!("tests/numbering.xml")),
    ];
    parts.extend_from_slice(extra);
    zip_of(&parts)
}

#[test]
fn reads_styles_lists_links_and_revisions() {
    let text = read(&package(&[]), &Limits::default()).unwrap();
    assert_eq!(
        text,
        "# Title\n\n## Inherited   heading\n\n### Outline\n\n\
         - first\n  1. nested\n     wrapped\n\nnot a list\n\n\
         ***both*** plain [link](https://example.com/a_b)7\n\n\
         anchor\n\nboxed\n\n\
         | wide | c |\n| --- | --- |\n| a\\|1<br>second | z |\n"
    );

    let Content::Docx(blocks) = content(&package(&[]), &Limits::default()).unwrap() else {
        panic!()
    };
    let Block::ListItem { level: 1, ordered: true, text } = &blocks[4] else {
        panic!("{:?}", blocks[4])
    };
    assert_eq!(text.text, "nested\nwrapped");
    let Block::Paragraph { text } = &blocks[6] else { panic!("{:?}", blocks[6]) };
    assert_eq!(text.text, "both plain link7");
    let link = Some("https://example.com/a_b".to_owned());
    let marks = [
        Mark { range: 0..4, bold: true, italic: true, link: None },
        Mark { range: 11..15, bold: false, italic: false, link },
    ];
    assert_eq!(text.marks, marks, "a run's edge spaces are left out of its stretch");
}

#[test]
fn finds_media() {
    let package = package(&[
        ("word/media/image1.gif", "GIF89a\x02\x00\x03\x00"),
        ("word/mediator.gif", "GIF89a"),
    ]);
    let mut found = Vec::new();
    images(&package, &Limits::default(), |image| {
        found.push((image.location, image.format, image.width));
        Ok(())
    })
    .unwrap();
    let location = ImageLocation::Part { name: "word/media/image1.gif".into(), sheet: None };
    assert_eq!(found, [(location, ImageFormat::Gif, Some(2))]);
}
