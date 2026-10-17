use font_test_data::{AHEM, TINOS_SUBSET};
use read_fonts::TableProvider;

use super::*;

#[test]
fn preserves_all_glyphs_and_tables_except_cmap_and_signature() {
    for original in [TINOS_SUBSET, AHEM] {
        let output = for_pdf(&TrueTypeFont::from_bytes(original).unwrap()).unwrap();
        let before = FontRef::new(original).unwrap();
        let after = FontRef::new(&output).unwrap();
        assert!(after.table_data(Tag::new(b"cmap")).is_none());
        assert!(after.table_data(Tag::new(b"DSIG")).is_none());
        assert_eq!(before.maxp().unwrap().num_glyphs(), after.maxp().unwrap().num_glyphs());
        for record in before.table_directory().table_records() {
            let tag = record.tag();
            if tag == Tag::new(b"cmap") || tag == Tag::new(b"DSIG") {
                continue;
            }
            let old = before.table_data(tag).unwrap();
            let new = after.table_data(tag).unwrap();
            if tag == Tag::new(b"head") {
                assert_eq!(&old.as_bytes()[..8], &new.as_bytes()[..8]);
                assert_eq!(&old.as_bytes()[12..], &new.as_bytes()[12..]);
            } else {
                assert_eq!(old.as_bytes(), new.as_bytes(), "{tag}");
            }
        }
    }
}

#[test]
fn directory_checksums_and_zero_padding_are_valid() {
    let output = for_pdf(&TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    let font = FontRef::new(&output).unwrap();
    let directory = font.table_directory();
    let count = directory.num_tables();
    assert_eq!(directory.search_range(), 16 * 2_u16.pow(u32::from(directory.entry_selector())));
    assert_eq!(directory.range_shift(), 16 * count - directory.search_range());
    let total = output
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0_u32, |sum, word| sum.wrapping_add(u32::from_be_bytes(*word)));
    assert_eq!(total, 0xB1B0_AFBA);
    let mut end = 12 + 16 * usize::from(count);
    for record in directory.table_records() {
        let start = record.offset() as usize;
        let length = record.length() as usize;
        assert_eq!(start, end.next_multiple_of(4));
        assert!(output[end..start].iter().all(|&byte| byte == 0));
        let mut table = output[start..start + length].to_vec();
        if record.tag() == Tag::new(b"head") {
            table[8..12].fill(0);
        }
        table.resize(table.len().next_multiple_of(4), 0);
        let sum = table
            .as_chunks::<4>()
            .0
            .iter()
            .fold(0_u32, |sum, word| sum.wrapping_add(u32::from_be_bytes(*word)));
        assert_eq!(record.checksum(), sum);
        end = start + length;
    }
    assert!(output[end..].iter().all(|&byte| byte == 0));
}

#[test]
fn keeps_legacy_truetype_signature_and_rejects_ambiguous_tables() {
    let mut original = TINOS_SUBSET.to_vec();
    original[..4].copy_from_slice(b"true");
    let output = for_pdf(&TrueTypeFont::from_bytes(&original).unwrap()).unwrap();
    assert!(output.starts_with(b"true"));

    // Give two unused tables the same tag, leaving all the loader's required
    // tables intact. The output directory must not contain ambiguous duplicates.
    let font = FontRef::new(TINOS_SUBSET).unwrap();
    let mut duplicates = TINOS_SUBSET.to_vec();
    for tag in [Tag::new(b"OS/2"), Tag::new(b"name")] {
        let index =
            font.table_directory().table_records().iter().position(|r| r.tag() == tag).unwrap();
        let offset = 12 + 16 * index;
        duplicates[offset..offset + 4].copy_from_slice(b"zzzz");
    }
    let font = TrueTypeFont::from_bytes(&duplicates).unwrap();
    assert!(for_pdf(&font).is_err());
}

#[test]
fn drops_a_signature_invalidated_by_rebuilding_the_font() {
    let font = FontRef::new(TINOS_SUBSET).unwrap();
    let index = font
        .table_directory()
        .table_records()
        .iter()
        .position(|record| record.tag() == Tag::new(b"name"))
        .unwrap();
    let mut data = TINOS_SUBSET.to_vec();
    let offset = 12 + 16 * index;
    data[offset..offset + 4].copy_from_slice(b"DSIG");
    let output = for_pdf(&TrueTypeFont::from_bytes(&data).unwrap()).unwrap();
    assert!(FontRef::new(&output).unwrap().table_data(Tag::new(b"DSIG")).is_none());
}
