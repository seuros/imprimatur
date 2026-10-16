use std::assert_matches;

use super::*;

#[test]
fn parses_cell_references() {
    const CASES: &[(&str, (u32, u16))] = &[
        ("A1", (0, 0)),
        ("b2", (1, 1)),
        ("Z10", (9, 25)),
        ("AA1", (0, 26)),
        ("XFD1048576", (1_048_575, 16_383)),
    ];
    for &(reference, expected) in CASES {
        assert_eq!(parse_cell_reference(reference).unwrap(), expected, "{reference}");
    }
    for reference in
        ["", "A", "1", "A0", "A01", "XFE1", "A1048577", "AAAA1", "$A$1", "A1B", "A 1", "É1"]
    {
        assert_matches!(
            parse_cell_reference(reference),
            Err(Error::InvalidRange(_)),
            "{reference:?}"
        );
    }
}

#[test]
fn parses_and_displays_ranges() {
    const CASES: &[(&str, &str)] =
        &[("B2", "B2"), ("B2:B2", "B2"), ("F2:F1000", "F2:F1000"), ("D9:b2", "B2:D9")];
    for &(text, shown) in CASES {
        assert_eq!(text.parse::<CellRange>().unwrap().to_string(), shown, "{text}");
    }
    for text in ["", "B2:", ":B2", "B2:C3:D4", "Sheet1!A1", "A:A"] {
        assert_matches!(text.parse::<CellRange>(), Err(Error::InvalidRange(_)), "{text:?}");
    }
    assert_matches!(CellRange::cell(MAX_ROWS, 0), Err(Error::RowOutOfRange(_)));
    assert_matches!(CellRange::new((0, 0), (0, MAX_COLUMNS)), Err(Error::ColumnOutOfRange(_)));
}

#[test]
fn detects_overlaps() {
    let range = |text: &str| text.parse::<CellRange>().unwrap();
    let column = range("B2:B10");
    for (other, overlaps) in
        [("B10", true), ("A1:C2", true), ("B11", false), ("C2:C10", false), ("A1:Z1", false)]
    {
        assert_eq!(column.overlaps(&range(other)), overlaps, "{other}");
        assert_eq!(range(other).overlaps(&column), overlaps, "{other}, reversed");
    }
}
