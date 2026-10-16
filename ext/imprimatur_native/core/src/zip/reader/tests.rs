use std::assert_matches;

use super::*;
use crate::read::Limits;
use crate::test_support::zip_of;

#[test]
fn reads_entries_by_case_insensitive_name() {
    let archive = zip_of(&[("xl/workbook.xml", "<workbook/>"), ("big.txt", &"ab".repeat(10_000))]);
    let reader = ZipReader::new(&archive).unwrap();
    let limits = Limits::default();
    let budget = Budget::new(&limits, archive.len()).unwrap();
    assert_eq!(reader.read("/XL/Workbook.xml", &budget).unwrap().unwrap(), b"<workbook/>");
    assert_eq!(reader.read("big.txt", &budget).unwrap().unwrap().len(), 20_000, "inflated");
    assert_eq!(reader.read("missing", &budget).unwrap(), None);
}

#[test]
fn stops_zip_bombs_and_corruption() {
    let archive = zip_of(&[("bomb.txt", &"0".repeat(1_000_000))]);
    let reader = ZipReader::new(&archive).unwrap();
    let limits = Limits::default().max_inflated(100_000);
    let budget = Budget::new(&limits, archive.len()).unwrap();
    assert_matches!(reader.read("bomb.txt", &budget), Err(Error::LimitExceeded(_)));

    let mut corrupt = zip_of(&[("a.txt", "hello world")]);
    corrupt[30 + 5 + 2] ^= 0xFF;
    let reader = ZipReader::new(&corrupt).unwrap();
    let limits = Limits::default();
    let budget = Budget::new(&limits, corrupt.len()).unwrap();
    assert_matches!(reader.read("a.txt", &budget), Err(Error::Malformed { .. }));

    assert_matches!(ZipReader::new(b"PK\x03\x04 not really").err(), Some(Error::Malformed { .. }));
}
