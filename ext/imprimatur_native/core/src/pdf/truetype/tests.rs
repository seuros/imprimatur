use std::io::{Cursor, ErrorKind};

use font_test_data::{AHEM, TINOS_SUBSET};

use super::*;
use crate::pdf::objects::push_string;
use crate::read::{Limits, markdown};

#[test]
fn embeds_all_glyphs_with_scaled_widths_and_valid_references() {
    let source = TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap();
    let a = source.glyph_id('A').unwrap();
    let mut font = EmbeddedFont::new(source).unwrap();
    let code = font.encode_glyph(a, "A").unwrap();
    assert_eq!(code, [0, 1], "CIDs are independent of glyph IDs");
    assert_eq!(font.encode_glyph(a, "A").unwrap(), code);
    let pdf = document(&font, &code);

    let parent = object_text(&pdf, 5);
    assert!(parent.contains("/Subtype /Type0 /BaseFont /Tinos-Regular /Encoding /Identity-H"));
    assert!(parent.contains("/DescendantFonts [6 0 R] /ToUnicode 10 0 R"));
    let descendant = object_text(&pdf, 6);
    assert!(descendant.contains("/Subtype /CIDFontType2 /BaseFont /Tinos-Regular"));
    assert!(descendant.contains("/FontDescriptor 7 0 R /DW 777.83 /W [1 [722.17 ]]"));
    assert!(descendant.contains("/CIDToGIDMap 9 0 R"));
    let descriptor = object_text(&pdf, 7);
    assert!(descriptor.contains("/FontName /Tinos-Regular"));
    assert!(
        descriptor.contains("/Ascent 891.11 /Descent -216.31 /CapHeight 654.79"),
        "{descriptor}"
    );
    assert!(descriptor.contains("/FontFile2 8 0 R"));
    let (dictionary, data) = stream(&pdf, 8);
    assert!(dictionary.contains(&format!("/Length1 {}", font.program.len())));
    assert_eq!(data, font.program, "the complete prepared font program");
    assert_eq!(stream(&pdf, 9).1, [0, 0, 0, 3]);
    assert_eq!(markdown(&pdf, &Limits::default()).unwrap(), "A\n");

    for id in 1..=11 {
        assert!(object(&pdf, id).starts_with(format!("{id} 0 obj\n").as_bytes()));
    }
    assert!(object_text(&pdf, 11).contains("/Producer (font test)"));
    assert_eq!(document(&font, &code), pdf, "deterministic serialization");
}

#[test]
fn shared_glyphs_retain_distinct_unicode_text() {
    let source = TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap();
    let space = source.glyph_id(' ').unwrap();
    assert_eq!(source.glyph_id('\u{a0}'), Some(space));
    let mut font = EmbeddedFont::new(source).unwrap();
    assert_eq!(font.encode_glyph(space, " ").unwrap(), [0, 1]);
    assert_eq!(font.encode_glyph(space, "\u{a0}").unwrap(), [0, 2]);
    assert_eq!(font.encode_glyph(space, " ").unwrap(), [0, 1]);
    let pdf = document(&font, &[0, 1, 0, 2]);
    assert_eq!(stream(&pdf, 9).1, [0, 0, 0, 1, 0, 1]);
    let cmap = font.to_unicode();
    assert!(cmap.contains("<0001> <0020>\n"));
    assert!(cmap.contains("<0002> <00A0>\n"));
}

#[test]
fn unicode_maps_support_ligatures_and_supplementary_characters() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    // These synthetic uses check mapping syntax and extraction, not the fixture's
    // outlines or Arabic shaping. The font has no Arabic or emoji glyphs.
    let mut text = Vec::new();
    for original in ["fi", "\u{1f600}", "A\u{301}", "لا"] {
        text.extend_from_slice(&font.encode_glyph(3, original).unwrap());
    }
    let cmap = font.to_unicode();
    assert!(cmap.contains("<0001> <00660069>\n"));
    assert!(cmap.contains("<0002> <D83DDE00>\n"));
    assert!(cmap.contains("<0003> <00410301>\n"));
    assert!(cmap.contains("<0004> <06440627>\n"));
    let pdf = document(&font, &text[..6]);
    assert_eq!(markdown(&pdf, &Limits::default()).unwrap(), "fi😀A\u{301}\n");
    assert_eq!(String::from_utf8(stream(&pdf, 10).1).unwrap(), cmap);
}

#[test]
fn zero_advance_and_unmapped_glyphs_are_preserved() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(AHEM).unwrap()).unwrap();
    assert_eq!(font.encode_glyph(1, "").unwrap(), [0, 1]);
    let pdf = document(&font, &[0, 1]);
    assert!(object_text(&pdf, 6).contains("/W [1 [0 ]]"));
    assert_eq!(stream(&pdf, 9).1, [0, 0, 0, 1]);
    assert!(!font.to_unicode().contains("beginbfchar"));
    assert!(font.to_unicode().contains("<0000> <FFFF>"));
}

#[test]
fn unused_font_has_an_empty_mapping_and_a_notdef_entry() {
    let font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    let pdf = document(&font, &[]);
    assert!(object_text(&pdf, 6).contains("/W []"));
    assert_eq!(stream(&pdf, 9).1, [0, 0]);
    assert_eq!(stream(&pdf, 8).1, font.program);
    assert!(!font.to_unicode().contains("beginbfchar"));
}

#[test]
fn unicode_mapping_blocks_have_at_most_one_hundred_entries() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    for index in 0..205 {
        font.encode_glyph(3, &format!("item{index}")).unwrap();
    }
    let cmap = font.to_unicode();
    assert_eq!(cmap.matches("100 beginbfchar\n").count(), 2);
    assert_eq!(cmap.matches("5 beginbfchar\n").count(), 1);
    assert_eq!(cmap.matches("endbfchar\n").count(), 3);
    assert!(cmap.contains("<00CD> <006900740065006D003200300034>\n"));
}

#[test]
fn rejects_invalid_glyphs_and_oversized_unicode_without_allocating_codes() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    for glyph in [0, 7, u16::MAX] {
        assert!(matches!(font.encode_glyph(glyph, "A"), Err(Error::Malformed { .. })));
    }
    assert!(font.encode_glyph(3, &"😀".repeat(129)).is_err());
    assert_eq!(font.encode_glyph(3, &"😀".repeat(128)).unwrap(), [0, 1]);
    assert_eq!(font.encode_glyph(3, "A").unwrap(), [0, 2]);
}

#[test]
fn full_code_space_keeps_existing_mappings_and_rejects_overflow() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    for index in 1..=u16::MAX {
        assert_eq!(font.encode_glyph(3, &index.to_string()).unwrap(), index.to_be_bytes());
    }
    assert!(font.encode_glyph(3, "overflow").is_err());
    assert_eq!(font.encode_glyph(3, "65535").unwrap(), [255, 255]);
    assert!(font.to_unicode().contains("<FFFF> <00360035003500330035>\n"));
    let pdf = document(&font, &[]);
    let widths = object_text(&pdf, 6).split("/W [").nth(1).unwrap();
    let widths = widths.split("] /CIDToGIDMap").next().unwrap();
    let mut total = 0;
    let mut arrays = 0;
    for group in widths.split('[').skip(1) {
        let count = group.split(']').next().unwrap().split_whitespace().count();
        assert!(count <= 8191, "PDF array implementation limit");
        total += count;
        arrays += 1;
    }
    assert_eq!(total, usize::from(u16::MAX));
    assert!(2 * arrays <= 8191, "the outer W array also has a limit");
}

#[test]
fn font_resources_can_follow_other_font_resources() {
    let mut font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    font.encode_glyph(3, "A").unwrap();
    let mut writer = PdfWriter::new(Vec::new()).unwrap();
    font.write(&mut writer, 1).unwrap();
    font.write(&mut writer, 1 + OBJECTS).unwrap();
    let pdf = writer.finish(1, 1).unwrap();
    assert!(object_text(&pdf, 7).contains("/DescendantFonts [8 0 R] /ToUnicode 12 0 R"));
    assert!(object_text(&pdf, 8).contains("/FontDescriptor 9 0 R"));
    assert!(object_text(&pdf, 9).contains("/FontFile2 10 0 R"));
    assert_eq!(stream(&pdf, 10).1, font.program);
}

#[test]
fn propagates_output_errors() {
    let font = EmbeddedFont::new(TrueTypeFont::from_bytes(TINOS_SUBSET).unwrap()).unwrap();
    let mut storage = [0; 250];
    let mut writer = PdfWriter::new(Cursor::new(storage.as_mut_slice())).unwrap();
    assert!(matches!(
        font.write(&mut writer, 1),
        Err(Error::Io(error)) if error.kind() == ErrorKind::WriteZero
    ));
}

fn document(font: &EmbeddedFont<'_>, codes: &[u8]) -> Vec<u8> {
    let mut pdf = PdfWriter::new(Vec::new()).unwrap();
    pdf.object(1, format_args!("<< /Type /Catalog /Pages 2 0 R >>")).unwrap();
    pdf.object(2, format_args!("<< /Type /Pages /Kids [3 0 R] /Count 1 >>")).unwrap();
    pdf.object(
        3,
        format_args!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
    )
    .unwrap();
    let mut content = b"BT /F1 12 Tf 30 200 Td ".to_vec();
    push_string(&mut content, codes);
    content.extend_from_slice(b" Tj ET");
    pdf.stream(4, &content, &[]).unwrap();
    font.write(&mut pdf, 5).unwrap();
    let info = 5 + OBJECTS;
    pdf.object(info, format_args!("<< /Producer (font test) >>")).unwrap();
    pdf.finish(1, info).unwrap()
}

fn object(pdf: &[u8], id: usize) -> &[u8] {
    let start = pdf.windows(10).rposition(|bytes| bytes == b"startxref\n").unwrap() + 10;
    let xref: usize =
        std::str::from_utf8(&pdf[start..]).unwrap().lines().next().unwrap().parse().unwrap();
    let table = std::str::from_utf8(&pdf[xref..]).unwrap();
    let offset = |line: &str| line[..10].parse::<usize>().unwrap();
    let begin = offset(table.lines().nth(id + 2).unwrap());
    let next = table.lines().nth(id + 3).unwrap();
    let end = if next == "trailer" { xref } else { offset(next) };
    &pdf[begin..end]
}

fn object_text(pdf: &[u8], id: usize) -> &str {
    std::str::from_utf8(object(pdf, id)).unwrap()
}

fn stream(pdf: &[u8], id: usize) -> (&str, Vec<u8>) {
    let object = object(pdf, id);
    let start = object.windows(7).position(|bytes| bytes == b"stream\n").unwrap() + 7;
    let dictionary = std::str::from_utf8(&object[..start]).unwrap();
    let length: usize = dictionary
        .split("/Length ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let decoded =
        miniz_oxide::inflate::decompress_to_vec_zlib(&object[start..start + length]).unwrap();
    (dictionary, decoded)
}
