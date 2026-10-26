use super::*;
use crate::read::{Budget, Limits};
use crate::test_support::pdf_of;

fn range(start: u32, style: Option<u8>, prefix: Option<&str>, first: i64) -> LabelRange {
    LabelRange { start, style, prefix: prefix.map(str::to_owned), first }
}

#[test]
fn formats_each_numeral_style() {
    let ranges = [range(0, Some(b'r'), None, 1)];
    assert_eq!(label_for(&ranges, 0).as_deref(), Some("i"));
    assert_eq!(label_for(&ranges, 8).as_deref(), Some("ix"));

    let ranges = [range(0, Some(b'R'), None, 1)];
    assert_eq!(label_for(&ranges, 3).as_deref(), Some("IV"));

    let ranges = [range(0, Some(b'D'), Some("A-"), 1)];
    assert_eq!(label_for(&ranges, 0).as_deref(), Some("A-1"));

    let ranges = [range(0, Some(b'a'), None, 1)];
    assert_eq!(label_for(&ranges, 25).as_deref(), Some("z"));
    assert_eq!(label_for(&ranges, 26).as_deref(), Some("aa"));

    let ranges = [range(0, Some(b'A'), None, 1)];
    assert_eq!(label_for(&ranges, 27).as_deref(), Some("BB"));

    let ranges = [range(0, None, Some("Draft"), 1)];
    assert_eq!(label_for(&ranges, 5).as_deref(), Some("Draft"));
}

#[test]
fn picks_the_range_that_covers_the_page() {
    let ranges = [range(0, Some(b'r'), None, 1), range(10, Some(b'D'), None, 1)];
    assert_eq!(label_for(&ranges, 9).as_deref(), Some("x"));
    assert_eq!(label_for(&ranges, 10).as_deref(), Some("1"));
    assert_eq!(label_for(&ranges, 15).as_deref(), Some("6"));
}

#[test]
fn reads_a_number_tree_from_the_catalog() {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R /PageLabels \
          << /Nums [0 << /S /r >> 2 << /S /D /St 1 >>] >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
    ];
    let pdf = pdf_of(&objects);
    let limits = Limits::default();
    let budget = Budget::new(&limits, pdf.len()).unwrap();
    let document = Document::open(&pdf, &budget).unwrap();
    let labels = document.page_labels(3).unwrap().unwrap();
    assert_eq!(labels, [Some("i".to_owned()), Some("ii".to_owned()), Some("1".to_owned())]);
}

#[test]
fn has_no_labels_without_a_page_labels_entry() {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
    ];
    let pdf = pdf_of(&objects);
    let limits = Limits::default();
    let budget = Budget::new(&limits, pdf.len()).unwrap();
    let document = Document::open(&pdf, &budget).unwrap();
    assert_eq!(document.page_labels(1).unwrap(), None);
}
