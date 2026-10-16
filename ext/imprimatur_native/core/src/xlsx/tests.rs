use std::assert_matches;

use super::*;
use crate::test_support::{part, text_parts};

/// Every part of the package as text.
fn parts(book: &Workbook) -> Vec<(String, String)> {
    text_parts(book.to_bytes().unwrap())
}

fn sales() -> Workbook {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Sales").unwrap();
    sheet.append_row(["Region", "Q1"]).unwrap();
    sheet.append_row([Cell::from("North & South"), Cell::from(12.5)]).unwrap();
    sheet.append_row([Cell::from("Total"), Cell::formula("=SUM(B2:B2)")]).unwrap();
    book.add_sheet("Notes").unwrap().write(2, 27, true).unwrap();
    book
}

#[test]
fn writes_a_complete_package() {
    let parts = parts(&sales());
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/sheet2.xml",
            "xl/styles.xml",
            "xl/sharedStrings.xml",
        ]
    );

    let types = part(&parts, "[Content_Types].xml");
    assert!(types.contains(r#"PartName="/xl/worksheets/sheet2.xml""#), "{types}");
    assert!(types.contains("spreadsheetml.sharedStrings+xml"), "{types}");

    let workbook = part(&parts, "xl/workbook.xml");
    assert!(workbook.contains(r#"<sheet name="Sales" sheetId="1" r:id="rId1"/>"#), "{workbook}");
    assert!(workbook.contains(r#"<sheet name="Notes" sheetId="2" r:id="rId2"/>"#), "{workbook}");

    let relationships = part(&parts, "xl/_rels/workbook.xml.rels");
    let relationship =
        |id: &str, kind: &str| format!(r#"Id="{id}" Type="{}/{kind}""#, crate::opc::RELATIONSHIPS);
    assert!(relationships.contains(&relationship("rId3", "styles")), "{relationships}");
    assert!(relationships.contains(&relationship("rId4", "sharedStrings")), "{relationships}");
}

#[test]
fn writes_cells() {
    let parts = parts(&sales());
    let sales = part(&parts, "xl/worksheets/sheet1.xml");
    assert!(sales.contains(r#"<dimension ref="A1:B3"/>"#), "{sales}");
    assert!(
        sales.contains(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#
        ),
        "{sales}"
    );
    assert!(sales.contains(r#"<c r="B2"><v>12.5</v></c>"#), "{sales}");
    assert!(sales.contains(r#"<c r="B3"><f>SUM(B2:B2)</f></c>"#), "{sales}");

    let notes = part(&parts, "xl/worksheets/sheet2.xml");
    assert!(notes.contains(r#"<dimension ref="AB3"/>"#), "{notes}");
    assert!(notes.contains(r#"<row r="3"><c r="AB3" t="b"><v>1</v></c></row>"#), "{notes}");

    let strings = part(&parts, "xl/sharedStrings.xml");
    assert!(strings.contains("<si><t>North &amp; South</t></si>"), "{strings}");
}

#[test]
fn leaves_out_the_string_table_without_text() {
    let mut book = Workbook::new();
    book.add_sheet("Numbers").unwrap().append_row([1, 2, 3]).unwrap();
    let parts = parts(&book);
    assert!(parts.iter().all(|(name, _)| name != "xl/sharedStrings.xml"));
    assert!(!part(&parts, "[Content_Types].xml").contains("sharedStrings"));
    assert!(!part(&parts, "xl/_rels/workbook.xml.rels").contains("sharedStrings"));
}

#[test]
fn an_empty_sheet_spans_a1() {
    let mut book = Workbook::new();
    book.add_sheet("Blank").unwrap();
    let parts = parts(&book);
    assert!(
        part(&parts, "xl/worksheets/sheet1.xml")
            .contains(r#"<dimension ref="A1"/><sheetData></sheetData>"#)
    );
}

#[test]
fn output_is_deterministic() {
    assert_eq!(sales().to_bytes().unwrap(), sales().to_bytes().unwrap());
}

#[test]
fn validates_sheet_names() {
    let mut book = Workbook::new();
    book.add_sheet("Sales").unwrap();
    assert_matches!(book.add_sheet("SALES"), Err(Error::DuplicateSheetName(_)));

    let rejected = [
        "",
        "x".repeat(32).as_str(),
        "a/b",
        "a[1]",
        "what?",
        "'quoted",
        "end'",
        "History",
        "tab\u{1}",
    ]
    .map(str::to_owned);
    for name in rejected {
        assert_matches!(
            book.add_sheet(name.clone()),
            Err(Error::InvalidSheetName { .. }),
            "{name:?}"
        );
    }
    let accepted =
        ["x".repeat(31), "Q1 'draft' v2".to_owned(), "Résumé".to_owned(), "😀".repeat(15)];
    for name in accepted {
        assert!(book.add_sheet(name.clone()).is_ok(), "{name:?}");
    }
    assert_matches!(
        book.add_sheet("😀".repeat(16)),
        Err(Error::InvalidSheetName { .. }),
        "32 UTF-16 units"
    );
}

#[test]
fn writes_styles_dates_and_widths() {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Styled").unwrap();
    sheet.append_row_with_format(["Day", "Amount"], &Format::new().bold()).unwrap();
    let money = Format::new().number_format("#,##0.00").unwrap();
    sheet.write(1, 0, DateTime::date(2023, 3, 15).unwrap()).unwrap();
    sheet.write_with_format(1, 1, 1234.5, &money).unwrap();
    sheet.set_column_width(0, 12.0).unwrap().set_column_width(1, 12.0).unwrap();
    sheet.set_column_width(3, 30.5).unwrap();

    let parts = parts(&book);
    let xml = part(&parts, "xl/worksheets/sheet1.xml");
    assert!(xml.contains(r#"<c r="A1" s="1" t="s"><v>0</v></c>"#), "{xml}");
    assert!(
        xml.contains(r#"<c r="A2" s="2"><v>45000</v></c><c r="B2" s="3"><v>1234.5</v></c>"#),
        "{xml}"
    );
    let cols = concat!(
        r#"<cols><col min="1" max="2" width="12" customWidth="1"/>"#,
        r#"<col min="4" max="4" width="30.5" customWidth="1"/></cols><sheetData>"#,
    );
    assert!(xml.contains(cols), "{xml}");
    assert!(part(&parts, "xl/styles.xml").contains(r#"<cellXfs count="4">"#));
    assert_eq!(book.sheets()[0].format_at(1, 1), Some(&money));
    assert_eq!(book.sheets()[0].format_at(0, 9), None);
}

#[test]
fn lays_out_right_to_left_sheets() {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("عربي").unwrap();
    sheet.set_right_to_left(true).append_row(["مرحبا", "שלום"]).unwrap();
    book.add_sheet("Left").unwrap();
    let parts = parts(&book);
    let rtl = part(&parts, "xl/worksheets/sheet1.xml");
    let view = r#"<sheetViews><sheetView rightToLeft="1" workbookViewId="0"/></sheetViews>"#;
    assert!(rtl.contains(view), "{rtl}");
    assert!(!part(&parts, "xl/worksheets/sheet2.xml").contains("sheetViews"));
    assert!(part(&parts, "xl/sharedStrings.xml").contains("<t>مرحبا</t>"));
    assert!(part(&parts, "xl/workbook.xml").contains(r#"<sheet name="عربي""#));
}

#[test]
fn refuses_an_empty_workbook() {
    assert_matches!(Workbook::new().to_bytes(), Err(Error::EmptyWorkbook));
}

#[test]
fn writes_comments_and_validations() {
    let mut book = Workbook::new();
    let template = book.add_sheet("Template").unwrap();
    template.append_row(["Name", "Unit"]).unwrap();
    template
        .set_comment(0, 0, Comment::new("Field: Name").unwrap().author("Instructions"))
        .unwrap();
    let units = Validation::list_from("Units!$A$2:$A$20").unwrap();
    template.add_validation("B2:B1000".parse().unwrap(), units).unwrap();
    book.add_sheet("Units").unwrap().append_row(["Unit"]).unwrap();
    let notes = book.add_sheet("Notes").unwrap();
    notes.set_comment(2, 1, Comment::new("later").unwrap()).unwrap();

    let parts = parts(&book);
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/_rels/sheet1.xml.rels",
            "xl/comments1.xml",
            "xl/drawings/vmlDrawing1.vml",
            "xl/worksheets/sheet2.xml",
            "xl/worksheets/sheet3.xml",
            "xl/worksheets/_rels/sheet3.xml.rels",
            "xl/comments3.xml",
            "xl/drawings/vmlDrawing3.vml",
            "xl/styles.xml",
            "xl/sharedStrings.xml",
        ]
    );

    let types = part(&parts, "[Content_Types].xml");
    let vml = format!(r#"<Default Extension="vml" ContentType="{}"/>"#, package::VML_TYPE);
    assert!(types.contains(&vml), "{types}");
    assert!(types.contains(r#"PartName="/xl/comments3.xml""#), "{types}");
    assert!(!types.contains("comments2"), "{types}");

    let relationships = part(&parts, "xl/worksheets/_rels/sheet1.xml.rels");
    let relationship = |id: &str, kind: &str, target: &str| {
        format!(r#"Id="{id}" Type="{}/{kind}" Target="{target}""#, crate::opc::RELATIONSHIPS)
    };
    assert!(relationships.contains(&relationship("rId1", "comments", "../comments1.xml")));
    let drawing = relationship("rId2", "vmlDrawing", "../drawings/vmlDrawing1.vml");
    assert!(relationships.contains(&drawing), "{relationships}");

    let template = part(&parts, "xl/worksheets/sheet1.xml");
    let tail = concat!(
        r#"</sheetData><dataValidations count="1"><dataValidation type="list" allowBlank="1" "#,
        r#"showErrorMessage="1" sqref="B2:B1000"><formula1>Units!$A$2:$A$20</formula1>"#,
        r#"</dataValidation></dataValidations><legacyDrawing r:id="rId2"/></worksheet>"#,
    );
    assert!(template.ends_with(tail), "{template}");
    assert!(!part(&parts, "xl/worksheets/sheet2.xml").contains("legacyDrawing"));
    assert!(part(&parts, "xl/comments1.xml").contains("<author>Instructions</author>"));
    let first = part(&parts, "xl/drawings/vmlDrawing1.vml");
    let third = part(&parts, "xl/drawings/vmlDrawing3.vml");
    assert!(first.contains(r#"data="1""#) && first.contains(r#"id="_x0000_s1025""#), "{first}");
    assert!(third.contains(r#"data="2""#) && third.contains(r#"id="_x0000_s2049""#), "{third}");
}
