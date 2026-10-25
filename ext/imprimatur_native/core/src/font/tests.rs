use font_test_data::{AHEM, TINOS_SUBSET};
use read_fonts::{FontRef, TableProvider, types::Tag};

use super::{FontMetrics, TrueTypeFont};
use crate::Error;
use crate::test_support::font::{
    offset as table_offset, record as table_record, replace as set_table,
};
use std::assert_matches;

#[test]
fn reads_static_font_metrics_and_unicode_glyphs() {
    let font = TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap();
    assert!(std::ptr::eq(font.as_bytes(), TINOS_SUBSET));
    assert_eq!(font.glyph_count(), 7);
    assert_eq!(
        font.metrics(),
        FontMetrics { units_per_em: 2048, ascender: 1825, descender: -443, line_gap: 87 }
    );
    for (character, glyph, advance) in [('A', 3, 1479), ('a', 5, 909), ('\u{a0}', 1, 512)] {
        assert_eq!(font.glyph_id(character), Some(glyph));
        assert_eq!(font.advance_width(glyph), Some(advance));
    }
    assert_eq!(font.glyph_id('ا'), None);
    assert_eq!(font.glyph_id('\u{10ffff}'), None);
    assert_eq!(font.advance_width(0), Some(1593));
    assert_eq!(font.advance_width(7), None);
    assert_eq!(font.advance_width(u16::MAX), None);
}

#[test]
fn preserves_zero_width_glyphs() {
    let font = TrueTypeFont::from_bytes(AHEM).unwrap();
    assert_eq!(font.advance_width(1), Some(0));
    assert_eq!(font.glyph_id('A'), Some(35));
    assert_eq!(font.advance_width(35), Some(1000));
}

#[test]
fn uses_the_last_long_metric_for_remaining_glyphs() {
    let mut bytes = TINOS_SUBSET.to_vec();
    // One advance shared by all seven glyphs, with six trailing side bearings.
    set_table(&mut bytes, *b"hmtx", &[3, 232, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let hhea = table_offset(&bytes, *b"hhea");
    bytes[hhea + 34..hhea + 36].copy_from_slice(&1_u16.to_be_bytes());
    let font = TrueTypeFont::from_bytes(&bytes).unwrap();
    assert_eq!(font.advance_width(0), Some(1000));
    assert_eq!(font.advance_width(6), Some(1000));
    assert_eq!(font.advance_width(7), None);

    set_table(&mut bytes, *b"hmtx", &[3, 232, 0, 0]);
    assert_malformed(&bytes);
}

#[test]
fn full_unicode_mapping_takes_priority_over_bmp_mapping() {
    let original = FontRef::new(TINOS_SUBSET).unwrap();
    let bmp = original.cmap().unwrap().subtable(0).unwrap();
    let bmp = match bmp {
        read_fonts::tables::cmap::CmapSubtable::Format4(table) => table.offset_data().as_bytes(),
        _ => panic!("expected the Tinos BMP cmap"),
    };
    // Synthetic mappings test Unicode lookup only, not Arabic glyph outlines or shaping.
    let full = cmap12(&[('A', 2), ('ا', 3), ('\u{1f600}', 4)]);
    let bmp_offset = 20_u32;
    let full_offset = bmp_offset + u32::try_from(bmp.len()).unwrap();
    let mut cmap = vec![0, 0, 0, 2, 0, 3, 0, 1]; // Windows BMP first
    cmap.extend_from_slice(&bmp_offset.to_be_bytes());
    cmap.extend_from_slice(&[0, 3, 0, 10]); // Windows full repertoire second
    cmap.extend_from_slice(&full_offset.to_be_bytes());
    cmap.extend_from_slice(bmp);
    cmap.extend_from_slice(&full);
    for (first, full) in [
        ((3_u16, 1_u16), (3_u16, 10_u16)), // full Windows over BMP Windows
        ((3, 0), (3, 10)),                 // ignore symbol map
        ((1, 0), (0, 4)),                  // ignore MacRoman map
        ((3, 1), (0, 6)),                  // full Unicode over BMP Windows
    ] {
        cmap[4..6].copy_from_slice(&first.0.to_be_bytes());
        cmap[6..8].copy_from_slice(&first.1.to_be_bytes());
        cmap[12..14].copy_from_slice(&full.0.to_be_bytes());
        cmap[14..16].copy_from_slice(&full.1.to_be_bytes());
        let mut bytes = TINOS_SUBSET.to_vec();
        set_table(&mut bytes, *b"cmap", &cmap);
        let font = TrueTypeFont::from_bytes(&bytes).unwrap();
        assert_eq!(font.glyph_id('A'), Some(2));
        assert_eq!(font.glyph_id('ا'), Some(3));
        assert_eq!(font.glyph_id('\u{1f600}'), Some(4));
        // Use one chosen cmap consistently, without falling back per character.
        assert_eq!(font.glyph_id('B'), None);
    }
}

#[test]
fn missing_and_out_of_range_mappings_are_not_glyphs() {
    let mut bytes = TINOS_SUBSET.to_vec();
    let subtable = cmap12(&[('A', 0), ('B', 7), ('C', 65536)]);
    let mut cmap = vec![0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 12];
    cmap.extend_from_slice(&subtable);
    set_table(&mut bytes, *b"cmap", &cmap);
    let font = TrueTypeFont::from_bytes(&bytes).unwrap();
    for character in ['A', 'B', 'C', 'D'] {
        assert_eq!(font.glyph_id(character), None);
    }
}

#[test]
fn rejects_legacy_or_symbol_only_cmaps() {
    for (platform, encoding) in [(1_u16, 0_u16), (3, 0)] {
        let mut bytes = TINOS_SUBSET.to_vec();
        let cmap = table_offset(&bytes, *b"cmap");
        bytes[cmap + 4..cmap + 6].copy_from_slice(&platform.to_be_bytes());
        bytes[cmap + 6..cmap + 8].copy_from_slice(&encoding.to_be_bytes());
        assert_matches!(
            TrueTypeFont::from_bytes(&bytes),
            Err(Error::UnsupportedFont("no supported Unicode cmap"))
        );
    }
}

#[test]
fn rejects_truncated_cmap_subtables_and_arrays() {
    let original = FontRef::new(TINOS_SUBSET).unwrap();
    let cmap = original.table_data(Tag::new(b"cmap")).unwrap().as_bytes();
    for end in [4, 10, 12, 20, cmap.len() - 1] {
        let mut bytes = TINOS_SUBSET.to_vec();
        set_table(&mut bytes, *b"cmap", &cmap[..end]);
        assert_malformed(&bytes);
    }
    // The cmap4 length includes its header, but not the claimed segment arrays.
    let mut truncated = cmap.to_vec();
    truncated[14..16].copy_from_slice(&16_u16.to_be_bytes());
    let mut bytes = TINOS_SUBSET.to_vec();
    set_table(&mut bytes, *b"cmap", &truncated);
    assert_malformed(&bytes);

    let mut full = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
    full.extend_from_slice(&cmap12(&[('ا', 1)]));
    // Claim two groups when only one exists.
    full[24..28].copy_from_slice(&2_u32.to_be_bytes());
    set_table(&mut bytes, *b"cmap", &full);
    assert_malformed(&bytes);
}

#[test]
fn rejects_unsupported_font_formats() {
    for bytes in [
        font_test_data::ttc::TTC,
        font_test_data::NOTO_SANS_JP_CFF,
        font_test_data::VAZIRMATN_VAR,
        b"wOFF",
        b"wOF2",
    ] {
        assert_matches!(TrueTypeFont::from_bytes(bytes), Err(Error::UnsupportedFont(_)));
    }
}

#[test]
fn rejects_empty_garbage_and_truncated_input() {
    for bytes in [b"".as_slice(), b"not a font", b"\0\x01\0\0", &TINOS_SUBSET[..100]] {
        assert_malformed(bytes);
    }
    assert_malformed(&TINOS_SUBSET[..TINOS_SUBSET.len() / 2]);
}

#[test]
fn rejects_missing_or_truncated_required_tables() {
    for tag in [*b"head", *b"maxp", *b"hhea", *b"hmtx", *b"cmap", *b"loca", *b"glyf"] {
        let mut bytes = TINOS_SUBSET.to_vec();
        let record = table_record(&bytes, tag);
        bytes[record..record + 4].copy_from_slice(b"zzzz");
        assert_malformed(&bytes);

        let mut bytes = TINOS_SUBSET.to_vec();
        bytes[record + 12..record + 16].copy_from_slice(&1_u32.to_be_bytes());
        assert_malformed(&bytes);
    }
}

#[test]
fn rejects_tables_outside_the_file() {
    for (offset, length) in [(u32::MAX, 1), (1, u32::MAX)] {
        let mut bytes = TINOS_SUBSET.to_vec();
        let record = table_record(&bytes, *b"head");
        bytes[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[record + 12..record + 16].copy_from_slice(&length.to_be_bytes());
        assert_malformed(&bytes);
    }
}

#[test]
fn rejects_invalid_metric_headers() {
    for (tag, relative_offset, value) in [
        (*b"head", 18, 0_u16),
        (*b"head", 18, 16385),
        (*b"head", 50, 2),
        (*b"maxp", 4, 0),
        (*b"hhea", 34, 0),
        (*b"hhea", 34, 8),
    ] {
        let mut bytes = TINOS_SUBSET.to_vec();
        let offset = table_offset(&bytes, tag) + relative_offset;
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        assert_malformed(&bytes);
    }
}

#[test]
fn validates_short_and_long_outline_offsets() {
    let original = FontRef::new(TINOS_SUBSET).unwrap();
    let loca = original.loca(None).unwrap();
    let long: Vec<u8> = (0..=7).flat_map(|i| loca.get_raw(i).unwrap().to_be_bytes()).collect();
    let mut bytes = TINOS_SUBSET.to_vec();
    set_table(&mut bytes, *b"loca", &long);
    let head = table_offset(&bytes, *b"head");
    bytes[head + 50..head + 52].copy_from_slice(&1_i16.to_be_bytes());
    assert!(TrueTypeFont::from_bytes(&bytes).is_ok());

    let offset = table_offset(&bytes, *b"loca");
    bytes[offset + 28..offset + 32].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_malformed(&bytes);

    let mut bytes = TINOS_SUBSET.to_vec();
    let offset = table_offset(&bytes, *b"loca");
    bytes[offset..offset + 2].copy_from_slice(&u16::MAX.to_be_bytes());
    assert_malformed(&bytes);
}

fn assert_malformed(bytes: &[u8]) {
    assert_matches!(
        TrueTypeFont::from_bytes(bytes),
        Err(Error::Malformed { format: "TrueType font", .. })
    );
}

fn cmap12(mappings: &[(char, u32)]) -> Vec<u8> {
    let count = u32::try_from(mappings.len()).unwrap();
    let mut bytes = vec![0, 12, 0, 0]; // format and reserved
    bytes.extend_from_slice(&(16 + 12 * count).to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes()); // language
    bytes.extend_from_slice(&count.to_be_bytes());
    for &(character, glyph) in mappings {
        let code = u32::from(character).to_be_bytes();
        bytes.extend_from_slice(&code); // first codepoint in group
        bytes.extend_from_slice(&code); // last codepoint in group
        bytes.extend_from_slice(&glyph.to_be_bytes());
    }
    bytes
}
