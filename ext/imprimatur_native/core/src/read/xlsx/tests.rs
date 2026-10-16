use super::numbers::{Shown, classify, push_general};
use crate::read::{
    Content, ImageFormat, ImageLocation, Limits, Note, Value, content, images, markdown as read,
};
use crate::test_support::zip_of;
use crate::xlsx::DateTime;
use crate::xlsx::{Cell, Comment, Format, Workbook};

#[test]
fn reads_what_the_writer_wrote() {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Sales | Q1").unwrap();
    sheet.append_row(["Region", "Amount", "Share", "Closed", "Done"]).unwrap();
    let percent = Format::new().number_format("0.0%").unwrap();
    let date = DateTime::date(2023, 3, 15).unwrap();
    sheet
        .append_row([
            Cell::from("North & West"),
            Cell::from(0.1 + 0.2),
            Cell::from(0.25),
            date.into(),
            true.into(),
        ])
        .unwrap();
    sheet.write_with_format(1, 2, 0.25, &percent).unwrap();
    sheet.append_row([Cell::from("Total"), Cell::formula("SUM(B2:B2)")]).unwrap();
    sheet.write(2, 3, date.at(9, 30, 0, 0).unwrap()).unwrap();
    sheet
        .set_comment(0, 0, Comment::new("Field: Region\nRequired").unwrap().author("Instructions"))
        .unwrap();
    book.add_sheet("Blank").unwrap();

    let bytes = book.to_bytes().unwrap();
    let text = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(
        text,
        "## Sales | Q1\n\n\
         | Region | Amount | Share | Closed | Done |\n\
         | --- | --- | --- | --- | --- |\n\
         | North & West | 0.3 | 25% | 2023-03-15 | TRUE |\n\
         | Total | =SUM(B2:B2) |  | 2023-03-15 09:30:00 |  |\n\n\
         Comments:\n\n\
         - A1 (Instructions): Field: Region\n  Required\n\n\
         ## Blank\n\n\
         _Empty._\n"
    );

    let Content::Xlsx(sheets) = content(&bytes, &Limits::default()).unwrap() else { panic!() };
    let values: Vec<(u32, u32, Value)> = sheets[0]
        .cells
        .cells()
        .iter()
        .filter(|cell| cell.row > 0)
        .map(|cell| (cell.row, cell.column, cell.value))
        .collect();
    let morning = date.at(9, 30, 0, 0).unwrap();
    assert_eq!(
        values,
        [
            (1, 0, Value::Text),
            (1, 1, Value::Number(0.1 + 0.2)),
            (1, 2, Value::Number(0.25)),
            (1, 3, Value::Date(date)),
            (1, 4, Value::Bool(true)),
            (2, 0, Value::Text),
            (2, 1, Value::Formula),
            (2, 3, Value::Date(morning)),
        ],
        "values keep what the text rounds or formats"
    );
    let note = Note {
        row: 0,
        column: 0,
        author: Some("Instructions".into()),
        text: "Field: Region\nRequired".into(),
    };
    assert_eq!(sheets[0].comments, [note]);
    assert_eq!(
        (sheets[1].name.as_str(), sheets[1].hidden, sheets[1].cells.is_empty()),
        ("Blank", false, true)
    );
}

/// A package of the parts in `tests/`, its workbook properties replaced,
/// and `extra` parts.
fn package(properties: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let workbook = include_str!("tests/workbook.xml").replace("<!--properties-->", properties);
    let mut parts = vec![
        ("[Content_Types].xml", include_str!("tests/content_types.xml")),
        ("_rels/.rels", include_str!("tests/package.rels")),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", include_str!("tests/workbook.xml.rels")),
        ("xl/worksheets/data.xml", include_str!("tests/data.xml")),
        ("xl/styles.xml", include_str!("tests/styles.xml")),
        ("xl/strings.xml", include_str!("tests/strings.xml")),
    ];
    parts.extend_from_slice(extra);
    zip_of(&parts)
}

#[test]
fn reads_what_other_writers_write() {
    let text = read(&package("", &[]), &Limits::default()).unwrap();
    assert_eq!(
        text,
        "## Data (hidden)\n\n\
         | rich text | inline | #N/A |  |  |\n\
         | --- | --- | --- | --- | --- |\n\
         | 2023-03-15 | 1.5 | 18:00:00 | 0 | cached |\n",
        "a quoted word is no date, [h] is a time, and 1900-01-00 never existed"
    );
    let text = read(&package(r#"<workbookPr date1904="1"/>"#, &[]), &Limits::default()).unwrap();
    assert!(
        text.contains("| 2027-03-16 | 1.5 | 18:00:00 | 1904-01-01 |"),
        "the 1904 epoch: {text}"
    );
}

#[test]
fn finds_media_and_the_sheets_showing_them() {
    let package = package(
        "",
        &[
            ("xl/worksheets/_rels/data.xml.rels", include_str!("tests/data.xml.rels")),
            ("xl/drawings/_rels/drawing1.xml.rels", include_str!("tests/drawing1.xml.rels")),
            ("xl/media/image1.gif", "GIF89a\x02\x00\x03\x00"),
            ("xl/media/unused.svg", "<svg/>"),
        ],
    );
    let mut found = Vec::new();
    images(&package, &Limits::default(), |image| {
        let ImageLocation::Part { name, sheet } = image.location else { panic!("{image:?}") };
        found.push((name, sheet, image.format, image.width, image.height));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        found,
        [
            ("xl/media/image1.gif".into(), Some("Data".into()), ImageFormat::Gif, Some(2), Some(3)),
            ("xl/media/unused.svg".into(), None, ImageFormat::Svg, None, None),
        ]
    );
}

#[test]
fn classifies_number_formats() {
    const CASES: &[(&str, Shown)] = &[
        ("General", Shown::General),
        ("0.00", Shown::General),
        ("0%", Shown::Percent),
        ("yyyy-mm-dd", Shown::Date),
        ("mmm", Shown::Date),
        ("h:mm AM/PM", Shown::Time),
        ("[h]:mm:ss", Shown::Time),
        ("dd/mm/yy hh:mm", Shown::DateTime),
        ("\"Day\" 0", Shown::General),
        ("[Red]0.00;[Blue]-0.00", Shown::General),
        ("#,##0.00\\ \"d\"", Shown::General),
    ];
    for &(code, shown) in CASES {
        assert_eq!(classify(code), shown, "{code}");
    }
}

#[test]
fn general_shows_fifteen_digits() {
    const CASES: &[(f64, &str)] = &[
        (0.1 + 0.2, "0.3"),
        (12.0, "12"),
        (-0.5, "-0.5"),
        (1e20, "100000000000000000000"),
        (1.0 / 3.0, "0.333333333333333"),
    ];
    let (mut out, mut scratch) = (String::new(), String::new());
    for &(number, shown) in CASES {
        out.clear();
        push_general(&mut out, number, &mut scratch);
        assert_eq!(out, shown, "{number}");
    }
}
