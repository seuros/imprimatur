use font_test_data::TINOS_SUBSET;

use super::*;

#[test]
fn respects_outline_embedding_permissions_without_subsetting() {
    for rights in [0_u16, 4, 8, 0x0100, 0x0006] {
        let mut data = TINOS_SUBSET.to_vec();
        set_u16(&mut data, *b"OS/2", 8, rights);
        assert!(Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()).is_ok());
    }
    for rights in [2_u16, 0x0200, 0x0204] {
        let mut data = TINOS_SUBSET.to_vec();
        set_u16(&mut data, *b"OS/2", 8, rights);
        assert!(matches!(
            Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()),
            Err(Error::UnsupportedFont(_))
        ));
    }
}

#[test]
fn missing_optional_metadata_has_documented_defaults() {
    let mut data = TINOS_SUBSET.to_vec();
    for tag in [*b"OS/2", *b"post"] {
        let record = record(&data, tag);
        data[record..record + 4].copy_from_slice(b"zzzz");
    }
    let descriptor = Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()).unwrap();
    assert_eq!(descriptor.cap_height.0, descriptor.ascent.0);
    assert_eq!(descriptor.italic_angle.0, 0.0);
    assert_eq!(descriptor.flags, 4);
}

#[test]
fn rejects_truncated_metadata_and_inverted_bounds() {
    for tag in [*b"OS/2", *b"post", *b"name"] {
        let mut data = TINOS_SUBSET.to_vec();
        let record = record(&data, tag);
        data[record + 12..record + 16].copy_from_slice(&1_u32.to_be_bytes());
        let font = TrueTypeFont::from_bytes(&data).unwrap();
        assert!(matches!(Descriptor::read(&font), Err(Error::Malformed { .. })));
    }
    let mut data = TINOS_SUBSET.to_vec();
    set_u16(&mut data, *b"head", 36, 32767); // xMin above xMax
    assert!(Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()).is_err());
}

#[test]
fn names_are_validated_and_escaped_for_pdf() {
    for (name, expected) in [("Tinos", "Tinos"), ("A#B", "A#23B")] {
        let data = with_name(name);
        let descriptor = Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()).unwrap();
        assert_eq!(descriptor.name, expected);
    }
    for name in ["", "Bad Name", "Bad/Name", "Bad%Name", "Bad(Name)", "عربي", &"A".repeat(64)] {
        let data = with_name(name);
        assert!(matches!(
            Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()),
            Err(Error::UnsupportedFont("no usable PostScript font name"))
        ));
    }
}

#[test]
fn rejects_odd_utf16_names_and_out_of_bounds_name_strings() {
    for (offset, value) in [(14, 1), (16, u16::MAX)] {
        let mut data = with_name("Example");
        set_u16(&mut data, *b"name", offset, value);
        let font = TrueTypeFont::from_bytes(&data).unwrap();
        assert!(matches!(Descriptor::read(&font), Err(Error::Malformed { .. })));
    }
}

#[test]
fn fixed_pitch_italic_and_serif_flags_come_from_font_metadata() {
    let mut data = TINOS_SUBSET.to_vec();
    set_u16(&mut data, *b"head", 44, 2); // italic macStyle
    set_u16(&mut data, *b"post", 14, 1); // low half of isFixedPitch
    set_u16(&mut data, *b"OS/2", 30, 0x0100); // serif family class
    let descriptor = Descriptor::read(&TrueTypeFont::from_bytes(&data).unwrap()).unwrap();
    assert_eq!(descriptor.flags, 4 | 1 | 2 | 64);
}

fn record(data: &[u8], tag: [u8; 4]) -> usize {
    let font = FontRef::new(data).unwrap();
    let index = font
        .table_directory()
        .table_records()
        .iter()
        .position(|record| record.tag() == Tag::new(&tag))
        .unwrap();
    12 + 16 * index
}

fn set_u16(data: &mut [u8], tag: [u8; 4], offset: usize, value: u16) {
    let record = record(data, tag);
    let start = u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize;
    data[start + offset..start + offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn with_name(name: &str) -> Vec<u8> {
    // A Windows Unicode PostScript name. Mutations leave checksums untouched,
    // like the font-loader tests; these fonts are never used as render fixtures.
    let mut data = TINOS_SUBSET.to_vec();
    let record = record(&data, *b"name");
    let encoded: Vec<u8> = name.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut table = vec![0, 0, 0, 1, 0, 18, 0, 3, 0, 1, 4, 9, 0, 6];
    table.extend_from_slice(&u16::try_from(encoded.len()).unwrap().to_be_bytes());
    table.extend_from_slice(&[0, 0]);
    table.extend_from_slice(&encoded);
    data.resize(data.len().next_multiple_of(4), 0);
    let start = u32::try_from(data.len()).unwrap();
    data[record + 8..record + 12].copy_from_slice(&start.to_be_bytes());
    data[record + 12..record + 16]
        .copy_from_slice(&u32::try_from(table.len()).unwrap().to_be_bytes());
    data.extend_from_slice(&table);
    data
}
