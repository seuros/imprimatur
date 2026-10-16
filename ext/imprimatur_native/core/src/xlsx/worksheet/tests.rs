use std::assert_matches;

use super::*;
use crate::xlsx::{DateTime, Validation, Workbook};

fn with_sheet(test: impl FnOnce(&mut Worksheet)) {
    let mut book = Workbook::new();
    test(book.add_sheet("Test").unwrap());
}

#[test]
fn appends_rows_after_the_last_written_row() {
    with_sheet(|sheet| {
        sheet.append_row(["a", "b"]).unwrap();
        sheet.write(4, 2, 1.5).unwrap();
        sheet.append_row(Vec::<Cell>::new()).unwrap(); // an empty row still advances
        sheet.append_row([true]).unwrap();
        sheet.append_sparse_row([None, Some(Cell::from(2_u8))], None).unwrap();

        assert_eq!(sheet.get(0, 1), Some(&Cell::from("b")));
        assert_eq!(sheet.get(4, 2), Some(&Cell::Number(1.5)));
        assert_eq!(sheet.get(6, 0), Some(&Cell::Bool(true)));
        assert_eq!(sheet.get(7, 0), None);
        assert_eq!(sheet.get(7, 1), Some(&Cell::Number(2.0)));
        assert_eq!(sheet.rows().map(|(row, _)| row).collect::<Vec<_>>(), [0, 4, 6, 7]);
        assert_eq!(sheet.next_row(), 8);
    });
}

#[test]
fn rejects_what_excel_would() {
    with_sheet(|sheet| {
        assert_matches!(sheet.write(MAX_ROWS, 0, 1.0), Err(Error::RowOutOfRange(1_048_576)));
        assert_matches!(sheet.write(0, MAX_COLUMNS, 1.0), Err(Error::ColumnOutOfRange(16_384)));
        assert_matches!(sheet.write(0, 0, f64::NAN), Err(Error::NonFiniteNumber(_)));
        assert_matches!(sheet.write(0, 0, f64::INFINITY), Err(Error::NonFiniteNumber(_)));
        assert_matches!(sheet.write(0, 0, "x".repeat(32_768)), Err(Error::TextTooLong(32_768)));
        assert_matches!(
            sheet.write(0, 0, Cell::formula("1".repeat(8_193))),
            Err(Error::FormulaTooLong(8_193))
        );
        assert!(
            sheet.write(0, 0, "é".repeat(32_767)).is_ok(),
            "the limit counts characters, not bytes"
        );
    });
}

#[test]
fn validates_column_widths() {
    with_sheet(|sheet| {
        sheet.set_column_width(0, 0.0).unwrap().set_column_width(1, 255.0).unwrap();
        for width in [-1.0, 255.5, f64::NAN, f64::INFINITY] {
            assert_matches!(
                sheet.set_column_width(2, width),
                Err(Error::InvalidColumnWidth(_)),
                "{width}"
            );
        }
        assert_matches!(sheet.set_column_width(MAX_COLUMNS, 1.0), Err(Error::ColumnOutOfRange(_)));
        assert_eq!(sheet.column_widths().len(), 2);
    });
}

#[test]
fn interns_each_format_once() {
    with_sheet(|sheet| {
        let bold = Format::new().bold();
        for row in 0..100 {
            sheet.write_with_format(row, 0, row, &bold).unwrap();
        }
        sheet.write_with_format(100, 0, 1, &Format::new()).unwrap();
        let date = DateTime::date(2026, 10, 15).unwrap();
        for row in 101..200 {
            sheet.write(row, 0, date).unwrap();
            sheet.write(row, 1, date.at(9, 0, 0, 0).unwrap()).unwrap();
        }
        let codes: Vec<Option<&str>> =
            sheet.formats().iter().map(|format| format.number_code.as_deref()).collect();
        assert_eq!(codes, [None, Some("yyyy-mm-dd"), Some("yyyy-mm-dd hh:mm:ss")]);
        assert_eq!(*sheet.formats()[0], bold);
        assert_eq!(sheet.format_at(150, 0), sheet.format_at(101, 0), "cached date style");
        assert_eq!(sheet.format_at(100, 0), None, "the default format is no format");
    });
}

#[test]
fn styles_each_cell_of_a_row() {
    with_sheet(|sheet| {
        let (fill, bold) = (Format::new().fill(crate::style::Color::WHITE), Format::new().bold());
        let cells = [
            (Some(Cell::from("a")), Some(&fill)),
            (None, Some(&bold)),
            (Some(Cell::from(1)), None),
        ];
        sheet.append_styled_row(cells).unwrap();
        assert_eq!(sheet.format_at(0, 0), Some(&fill));
        assert_eq!(sheet.get(0, 1), None, "a format alone writes no cell");
        assert_eq!(sheet.format_at(0, 2), None);
        assert_eq!(sheet.formats().len(), 1, "only formats of written cells are interned");
    });
}

#[test]
fn a_rejected_row_writes_nothing() {
    with_sheet(|sheet| {
        assert_matches!(sheet.append_row([Cell::from("ok"), Cell::Number(f64::NAN)]), Err(_));
        assert_eq!(sheet.rows().count(), 0);
        assert_eq!(sheet.next_row(), 0);
        assert_matches!(
            sheet.append_row(vec![0_u8; usize::from(MAX_COLUMNS) + 1]),
            Err(Error::ColumnOutOfRange(_))
        );
    });
}

#[test]
fn refuses_overlapping_validations() {
    with_sheet(|sheet| {
        let range = |text: &str| text.parse::<CellRange>().unwrap();
        let yes_no = Validation::list(["Yes", "No"]).unwrap();
        sheet.add_validation(range("B2:B100"), yes_no.clone()).unwrap();
        sheet.add_validation(range("C2"), yes_no.clone()).unwrap();
        assert_matches!(
            sheet.add_validation(range("A50:C50"), yes_no),
            Err(Error::OverlappingValidation(taken)) if taken == "A50:C50"
        );
        assert_eq!(sheet.validations().len(), 2, "a refused rule is not kept");
    });
}

#[test]
fn sets_comments_on_any_cell() {
    with_sheet(|sheet| {
        sheet.set_comment(0, 0, Comment::new("first").unwrap()).unwrap();
        sheet.set_comment(0, 0, Comment::new("replaced").unwrap()).unwrap();
        sheet.set_comment(9, 3, Comment::new("on an empty cell").unwrap()).unwrap();
        assert_eq!(sheet.comment(0, 0).map(Comment::text), Some("replaced"));
        assert_eq!(sheet.comments().len(), 2);
        assert_eq!(sheet.rows().count(), 0, "comments write no cells");
        assert_matches!(
            sheet.set_comment(MAX_ROWS, 0, Comment::new("x").unwrap()),
            Err(Error::RowOutOfRange(_))
        );
    });
}
