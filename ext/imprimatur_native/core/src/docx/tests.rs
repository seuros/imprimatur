use crate::doc::{Document, Paragraph, Run, Table};
use crate::opc::RELATIONSHIPS;
use crate::style::{Align, Color, TextDirection};
use crate::test_support::{part, text_parts};

fn parts(doc: &Document) -> Vec<(String, String)> {
    text_parts(doc.to_docx().unwrap())
}

fn sample() -> Document {
    let mut doc = Document::new();
    doc.heading(1, "Report").unwrap();
    doc.push_paragraph(
        Paragraph::new()
            .text("Plain ")
            .run(Run::new("bold red").bold().color(Color::rgb(0xC0_00_00)))
            .text(" a\tb\nc")
            .align(Align::Center),
    )
    .unwrap();
    doc.push_paragraph(Paragraph::new().text("שלום").direction(TextDirection::RightToLeft))
        .unwrap();
    doc.page_break();
    doc.table(Table::new([["Region", "Q1"], ["North", "12.5"]]).header()).unwrap();
    doc
}

#[test]
fn writes_a_complete_package() {
    let parts = parts(&sample());
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "word/styles.xml"
        ]
    );
    assert!(part(&parts, "[Content_Types].xml").contains("wordprocessingml.document.main+xml"));
    assert!(part(&parts, "word/styles.xml").contains(r#"w:styleId="Heading1""#));
}

#[test]
fn writes_paragraphs_runs_and_breaks() {
    let parts = parts(&sample());
    let body = part(&parts, "word/document.xml");
    for expected in [
        r#"<w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">Report</w:t>"#,
        r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#,
        r#"<w:rPr><w:b/><w:color w:val="C00000"/></w:rPr><w:t xml:space="preserve">bold red</w:t>"#,
        r#"<w:t xml:space="preserve"> a</w:t><w:tab/><w:t xml:space="preserve">b</w:t>"#,
        r#"<w:br/><w:t xml:space="preserve">c</w:t>"#,
        "<w:pPr><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr>",
        r#"<w:br w:type="page"/>"#,
        "<w:trPr><w:tblHeader/></w:trPr>",
        r#"<w:shd w:val="clear" w:color="auto" w:fill="D9E2F3"/>"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
    ] {
        assert!(body.contains(expected), "missing {expected}\n{body}");
    }
    assert!(body.contains("</w:tbl><w:p/><w:sectPr>"), "a table never ends the body");
}

#[test]
fn numbers_pages_in_a_footer() {
    let mut doc = sample();
    doc.set_page_numbers(true).set_page_size(crate::doc::PageSize::Letter);
    let parts = parts(&doc);
    let footer = part(&parts, "word/footer1.xml");
    assert!(
        footer.contains(r#"<w:instrText xml:space="preserve"> PAGE </w:instrText>"#),
        "{footer}"
    );
    assert!(
        footer.contains(r#"<w:instrText xml:space="preserve"> NUMPAGES </w:instrText>"#),
        "{footer}"
    );
    let body = part(&parts, "word/document.xml");
    let section = concat!(
        r#"<w:sectPr><w:footerReference w:type="default" r:id="rId2"/>"#,
        r#"<w:pgSz w:w="12240" w:h="15840"/>"#,
    );
    assert!(body.contains(section), "{body}");
    let footer = format!(r#"Id="rId2" Type="{RELATIONSHIPS}/footer" Target="footer1.xml""#);
    assert!(part(&parts, "word/_rels/document.xml.rels").contains(&footer));
    assert!(part(&parts, "[Content_Types].xml").contains("/word/footer1.xml"));
}

#[test]
fn output_is_deterministic() {
    assert_eq!(sample().to_docx().unwrap(), sample().to_docx().unwrap());
}
