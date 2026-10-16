use super::*;
use crate::xlsx::{DateTime, TextDirection, VerticalAlign, Workbook};

fn stylesheet(build: impl FnOnce(&mut Workbook)) -> (String, Vec<Vec<u32>>) {
    let mut book = Workbook::new();
    build(&mut book);
    let mut styles = Stylesheet::default();
    let remaps = book.sheets().iter().map(|sheet| styles.add_sheet(sheet).unwrap()).collect();
    (styles.to_xml(), remaps)
}

#[test]
fn the_default_stylesheet_has_what_excel_requires() {
    let (xml, remaps) = stylesheet(|book| {
        book.add_sheet("Plain").unwrap().append_row(["a"]).unwrap();
    });
    assert_eq!(remaps, [vec![0]]);
    assert!(!xml.contains("numFmts"), "{xml}");
    assert!(xml.contains(r#"<fills count="2">"#), "{xml}");
    assert!(xml.contains(r#"<cellXfs count="1">"#), "{xml}");
    assert!(xml.contains(r#"<cellStyle name="Normal" xfId="0" builtinId="0"/>"#), "{xml}");
}

#[test]
fn shares_formats_across_sheets() {
    let bold = Format::new().bold();
    let money = Format::new().number_format("#,##0.00").unwrap().fill(Color::rgb(0xFF_F2_CC));
    let (xml, remaps) = stylesheet(|book| {
        let first = book.add_sheet("One").unwrap();
        first.append_row_with_format(["Header"], &bold).unwrap();
        first.append_row_with_format([1.5], &money).unwrap();
        let second = book.add_sheet("Two").unwrap();
        second.append_row_with_format([2.5], &money).unwrap();
        second.write_with_format(1, 0, "x", &Format::new().italic().border(Border::Thin)).unwrap();
    });
    assert_eq!(remaps, [vec![0, 1, 2], vec![0, 2, 3]]);
    assert!(
        xml.contains(
            r##"<numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00"/></numFmts>"##
        ),
        "{xml}"
    );
    assert!(xml.contains(r#"<fonts count="3">"#), "default, bold, italic: {xml}");
    assert!(xml.contains(r#"<font><b/><sz val="11"/>"#), "{xml}");
    assert!(xml.contains(r#"<fgColor rgb="FFFFF2CC"/>"#), "{xml}");
    assert!(xml.contains(r#"<left style="thin"><color auto="1"/></left>"#), "{xml}");
    assert!(xml.contains(r#"<cellXfs count="4">"#), "{xml}");
    let money = concat!(
        r#"<xf numFmtId="164" fontId="0" fillId="2" borderId="0" xfId="0" "#,
        r#"applyNumberFormat="1" applyFill="1"/>"#,
    );
    assert!(xml.contains(money), "{xml}");
}

#[test]
fn writes_alignment_and_general() {
    let (xml, _) = stylesheet(|book| {
        let sheet = book.add_sheet("A").unwrap();
        let centered = Format::new().align(Align::Center).wrap().number_format("General").unwrap();
        sheet.write_with_format(0, 0, "x", &centered).unwrap();
        let arabic = Format::new().direction(TextDirection::RightToLeft);
        sheet.write_with_format(1, 0, "مرحبا", &arabic).unwrap();
    });
    assert!(!xml.contains("numFmts"), "General is built in: {xml}");
    assert!(
        xml.contains(r#"applyAlignment="1"><alignment horizontal="center" wrapText="1"/></xf>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"applyAlignment="1"><alignment readingOrder="2"/></xf>"#), "{xml}");
}

#[test]
fn dates_get_a_date_format_unless_they_have_one() {
    let date = DateTime::date(2023, 3, 15).unwrap();
    let (xml, _) = stylesheet(|book| {
        let sheet = book.add_sheet("Dates").unwrap();
        sheet.append_row([date, date.at(9, 30, 0, 0).unwrap()]).unwrap();
        sheet.append_row_with_format([date], &Format::new().bold()).unwrap();
        sheet
            .append_row_with_format([date], &Format::new().number_format("d mmm yyyy").unwrap())
            .unwrap();
        sheet.append_row([date]).unwrap();
    });
    for code in ["yyyy-mm-dd", "yyyy-mm-dd hh:mm:ss", "d mmm yyyy"] {
        assert!(xml.contains(&format!(r#"formatCode="{code}""#)), "{code}: {xml}");
    }
    assert!(xml.contains(r#"<cellXfs count="5">"#), "date, date-time, bold date, custom: {xml}");
}

#[test]
fn writes_font_names_sizes_and_vertical_alignment() {
    let (xml, _) = stylesheet(|book| {
        let sheet = book.add_sheet("Fonts").unwrap();
        let arial = Format::new().font_name("Arial").unwrap().font_size(10.0).unwrap();
        sheet.write_with_format(0, 0, "a", &arial).unwrap();
        let header = arial.clone().bold().align(Align::Center).valign(VerticalAlign::Center);
        sheet.write_with_format(1, 0, "b", &header).unwrap();
        sheet.write_with_format(2, 0, "c", &Format::new().font_size(12.5).unwrap()).unwrap();
    });
    assert!(xml.contains(r#"<fonts count="4">"#), "{xml}");
    assert!(
        xml.contains(r#"<font><sz val="11"/><name val="Calibri"/><family val="2"/></font>"#),
        "the default: {xml}"
    );
    assert!(xml.contains(r#"<font><sz val="10"/><name val="Arial"/></font>"#), "{xml}");
    assert!(xml.contains(r#"<font><b/><sz val="10"/><name val="Arial"/></font>"#), "{xml}");
    assert!(xml.contains(r#"<font><sz val="12.5"/><name val="Calibri"/><family val="2"/>"#));
    assert!(xml.contains(r#"<alignment horizontal="center" vertical="center"/>"#), "{xml}");
}
