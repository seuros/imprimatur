use crate::barcode::Barcode;
use crate::doc::{Document, Paragraph, Picture, Run, Symbol, Table};
use crate::opc::RELATIONSHIPS;
use crate::read::{PngColor, png};
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
    let logo = png(2, 1, PngColor::Gray, 8, None, &[0, 255]);
    doc.picture(Picture::new(logo).unwrap().width(72.0).unwrap().align(Align::Center));
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
            "word/styles.xml",
            "word/media/image1.png"
        ]
    );
    let types = part(&parts, "[Content_Types].xml");
    assert!(types.contains("wordprocessingml.document.main+xml"));
    assert!(types.contains(r#"<Default Extension="png" ContentType="image/png"/>"#), "{types}");
    let relationships = part(&parts, "word/_rels/document.xml.rels");
    assert!(relationships.contains(r#"Id="rId2" Type="#), "{relationships}");
    assert!(relationships.contains(r#"/image" Target="media/image1.png""#), "{relationships}");
    // 72 by 36 points, in EMUs, centered, its file rId2.
    let body = part(&parts, "word/document.xml");
    for expected in [
        r#"<w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:drawing><wp:inline"#,
        r#"<wp:extent cx="914400" cy="457200"/>"#,
        r#"<a:blip r:embed="rId2"/>"#,
    ] {
        assert!(body.contains(expected), "missing {expected}\n{body}");
    }
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

#[test]
fn sets_the_default_font_size_for_latin_and_complex_scripts() {
    let mut doc = sample();
    let original = parts(&doc);
    let styles = part(&original, "word/styles.xml");
    let default_size = r#"<w:sz w:val="22"/><w:szCs w:val="22"/>"#;
    assert!(styles.contains(default_size), "{styles}");
    for (points, half_points) in [(1.0, 2), (10.0, 20), (10.5, 21), (1638.0, 3276)] {
        doc.set_font_size(points).unwrap();
        let sized = parts(&doc);
        let size = format!(r#"<w:sz w:val="{half_points}"/><w:szCs w:val="{half_points}"/>"#);
        assert_eq!(part(&sized, "word/styles.xml"), styles.replacen(default_size, &size, 1));
        assert_eq!(part(&sized, "word/document.xml"), part(&original, "word/document.xml"));
    }
}

#[test]
fn takes_direction_from_the_text() {
    let mut doc = Document::new();
    doc.heading(1, "تقرير المطابقة").unwrap();
    doc.paragraph("شهادة صادرة لشركة الأمل للتجارة").unwrap();
    doc.push_paragraph(Paragraph::new().text("Supplier: ").text("شركة الأمل")).unwrap();
    doc.push_paragraph(
        Paragraph::new().text("مرحبا").text(" Foxconn").direction(TextDirection::LeftToRight),
    )
    .unwrap();
    doc.table(Table::new([["المنتج", "الكمية"], ["قماش", "100"]])).unwrap();
    let parts = parts(&doc);
    let body = part(&parts, "word/document.xml");
    let rtl = "<w:r><w:rPr><w:rtl/></w:rPr>";
    let latin = r#"<w:r><w:t xml:space="preserve">"#;
    for expected in [
        // A heading and a paragraph in Arabic, without being told.
        format!(r#"<w:pPr><w:pStyle w:val="Heading1"/><w:bidi/></w:pPr>{rtl}"#),
        format!("<w:p><w:pPr><w:bidi/></w:pPr>{rtl}"),
        // An English paragraph: its Arabic run alone right to left.
        format!("<w:p>{latin}Supplier: </w:t></w:r>{rtl}"),
        // Told left to right: no w:bidi, the Arabic run still right to left.
        format!("<w:p>{rtl}<w:t xml:space=\"preserve\">مرحبا</w:t></w:r>{latin} Foxconn"),
        // A mostly Arabic table, first column on the right; a cell of figures
        // in the table's direction.
        r#"<w:tblStyle w:val="TableGrid"/><w:bidiVisual/><w:tblW"#.to_owned(),
        format!("<w:p><w:pPr><w:bidi/></w:pPr>{rtl}<w:t xml:space=\"preserve\">100</w:t>"),
        // A mostly Arabic document: pages laid out right to left.
        concat!(
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>"#,
            "<w:bidi/></w:sectPr>"
        )
        .to_owned(),
    ] {
        assert!(body.contains(&expected), "missing {expected}\n{body}");
    }

    let sample = self::parts(&sample());
    let latin = part(&sample, "word/document.xml");
    assert!(!latin.contains("bidiVisual") && !latin.contains("<w:bidi/></w:sectPr>"), "{latin}");
}

#[test]
fn puts_barcodes_in_as_pictures() {
    let mut doc = Document::new();
    let ean = Barcode::ean13("9506000134352").unwrap();
    doc.barcode(Symbol::new(ean).width(113.0).unwrap().height(50.0).unwrap().align(Align::Center));
    let parts = parts(&doc);
    assert!(parts.iter().any(|(name, _)| name == "word/media/image1.png"));
    // 113 by 50 points, in EMUs.
    let body = part(&parts, "word/document.xml");
    assert!(body.contains(r#"<wp:extent cx="1435100" cy="635000"/>"#), "{body}");
    // One module a point: 5 pixels a module, for 300 an inch.
    let png = &doc.to_docx().unwrap();
    let image = crate::test_support::read_zip(png.clone())
        .into_iter()
        .find(|(name, _)| name == "word/media/image1.png")
        .unwrap()
        .1;
    let picture = Picture::new(image).unwrap();
    assert_eq!(picture.pixels, (113 * 5, 50 * 5));
}
