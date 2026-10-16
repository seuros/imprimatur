use super::*;
use crate::test_support::{png_of, tiff_of};

type Size = Option<(u32, u32)>;
/// TIFF tags and their values.
type Tags = [(u16, u32); 3];

#[test]
fn sniffs_formats_by_content_then_name() {
    let mut emf = vec![1, 0, 0, 0];
    emf.resize(40, 0);
    emf.extend_from_slice(b" EMF");
    let cases: &[(&[u8], &str, ImageFormat)] = &[
        (b"\x89PNG\r\n", "image1", ImageFormat::Png),
        (b"\xFF\xD8\xFF\xE0", "renamed.png", ImageFormat::Jpeg),
        (b"GIF89a", "", ImageFormat::Gif),
        (b"BM\0\0", "", ImageFormat::Bmp),
        (b"II*\0", "", ImageFormat::Tiff),
        (b"MM\0*", "", ImageFormat::Tiff),
        (b"RIFF\0\0\0\0WEBPVP8 ", "", ImageFormat::Webp),
        (b"\xD7\xCD\xC6\x9A", "", ImageFormat::Wmf),
        (b"\0\0\0\x0CjP  ", "", ImageFormat::Jpeg2000),
        (b"\xFF\x4F\xFF\x51", "", ImageFormat::Jpeg2000),
        (&emf, "", ImageFormat::Emf),
        (b"<svg/>", "media/logo.SVG", ImageFormat::Svg),
        (b"\xEF\xBB\xBF <?xml version=\"1.0\"?>\n<svg>", "image6", ImageFormat::Svg),
        (b"<html><body><svg>", "page", ImageFormat::Other),
        (b"", "image2.emf", ImageFormat::Emf),
        (b"???", "image3.wmf", ImageFormat::Wmf),
        (b"???", "image4.dat", ImageFormat::Other),
        (b"???", "image5", ImageFormat::Other),
    ];
    for (data, name, format) in cases {
        assert_eq!(ImageFormat::sniff(data, name), *format, "{name:?} {data:?}");
    }
}

/// Headers of every format whose size is read, and the size.
fn headers() -> Vec<(ImageFormat, Vec<u8>, Size)> {
    let concat = |parts: &[&[u8]]| parts.concat();
    let mut bmp = b"BM".to_vec();
    bmp.resize(18, 0);
    bmp.extend_from_slice(&concat(&[&4_i32.to_le_bytes(), &(-3_i32).to_le_bytes()]));
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
    jpeg.resize(20, 0);
    // A Huffman table sits among the start-of-frame markers but is none.
    jpeg.extend_from_slice(&[0xFF, 0xC4, 0, 7, 0x10, 0, 9, 0, 9]);
    jpeg.extend_from_slice(&[0xFF, 0xC2, 0, 17, 8, 0x01, 0xE0, 0x02, 0x80, 3]);
    jpeg.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9]);
    let big_endian_tiff = concat(&[
        b"MM\0*\0\0\0\x08\0\x02",
        &[1, 0, 0, 3, 0, 0, 0, 1, 0, 16, 0, 0],
        &[1, 1, 0, 4, 0, 0, 0, 1, 0, 0, 0, 32, 0, 0, 0, 0],
    ]);
    let webp =
        |chunk: &[u8], payload: &[u8]| concat(&[b"RIFF\0\0\0\0WEBP", chunk, &[0; 4], payload]);
    // Width - 1 in the low 14 bits, height - 1 in the next 14.
    let lossless = (99_u32 + (49 << 14)).to_le_bytes();
    let jp2 = concat(&[
        b"\0\0\0\x0CjP  \r\n\x87\n\0\0\0\x14ftypjp2 \0\0\0\0jp2 ",
        b"\0\0\0\x1Ejp2h\0\0\0\x16ihdr",
        &[0, 0, 1, 0xE0, 0, 0, 2, 0x80, 0, 3, 7, 7, 0, 0],
    ]);
    let codestream =
        concat(&[&[0xFF, 0x4F, 0xFF, 0x51, 0, 0x29, 0, 0], &[0, 0, 1, 0x10, 0, 0, 0, 0x90]]);
    let codestream = concat(&[&codestream, &16_u32.to_be_bytes(), &16_u32.to_be_bytes()]);
    let mut emf = concat(&[&1_u32.to_le_bytes(), &108_u32.to_le_bytes()]);
    for value in [0_i32, 0, 99, 49] {
        emf.extend_from_slice(&value.to_le_bytes());
    }
    emf.resize(40, 0);
    emf.extend_from_slice(b" EMF");
    let mut wmf = b"\xD7\xCD\xC6\x9A\0\0".to_vec();
    for value in [0_i16, 0, 1440, 720, 1440] {
        wmf.extend_from_slice(&value.to_le_bytes());
    }
    vec![
        (ImageFormat::Png, png(3, 2, PngColor::Gray, 8, None, &[]), Some((3, 2))),
        (ImageFormat::Png, b"\x89PNG".to_vec(), None),
        (ImageFormat::Gif, b"GIF89a\x05\x00\x07\x00".to_vec(), Some((5, 7))),
        (ImageFormat::Bmp, bmp, Some((4, 3))),
        (ImageFormat::Jpeg, jpeg, Some((640, 480))),
        (ImageFormat::Jpeg, b"\xFF\xD8\x00".to_vec(), None),
        (ImageFormat::Tiff, tiff(1728, 2, Fax::Group4, true, false, b""), Some((1728, 2))),
        (ImageFormat::Tiff, big_endian_tiff, Some((16, 32))),
        (ImageFormat::Tiff, b"II*\0".to_vec(), None),
        (ImageFormat::Webp, webp(b"VP8 ", b"\0\0\0\x9D\x01\x2A\x40\x01\xF0\x00"), Some((320, 240))),
        (ImageFormat::Webp, webp(b"VP8L", &concat(&[b"\x2F", &lossless])), Some((100, 50))),
        (ImageFormat::Webp, webp(b"VP8X", b"\0\0\0\0\x7F\x02\0\xDF\x01\0"), Some((640, 480))),
        (ImageFormat::Jpeg2000, jp2, Some((640, 480))),
        (ImageFormat::Jpeg2000, codestream, Some((256, 128))),
        (ImageFormat::Emf, emf, Some((100, 50))),
        (ImageFormat::Wmf, wmf, Some((96, 48))),
        (
            ImageFormat::Svg,
            b"<?xml version=\"1.0\"?>\n<!DOCTYPE svg>\n<svg width=\"120\" height='0.5in'>".to_vec(),
            Some((120, 48)),
        ),
        (
            ImageFormat::Svg,
            b"<svg viewBox=\"0 0 300 150\" width=\"100%\">".to_vec(),
            Some((300, 150)),
        ),
        (ImageFormat::Svg, b"<svg width=\"10em\" height=\"5em\">".to_vec(), None),
    ]
}

#[test]
fn reads_dimensions_from_headers() {
    for (format, data, size) in headers() {
        assert_eq!(format.dimensions(&data), size, "{format:?} {data:?}");
    }
}

#[test]
fn writes_pngs_row_by_row() {
    let gray = png_of(&png(3, 2, PngColor::Gray, 8, None, &[1, 2, 3, 4]));
    assert_eq!((gray.width, gray.height, gray.bits, gray.color), (3, 2, 8, 0));
    assert_eq!(gray.rows, [1, 2, 3, 4, 0, 0], "short data is padded");
    assert_eq!(gray.palette, None);

    let palette = [255, 0, 0, 0, 0, 255];
    let indexed = png_of(&png(9, 1, PngColor::Palette, 1, Some(&palette), &[0x80, 0x80, 0xFF]));
    assert_eq!((indexed.bits, indexed.color), (1, 3));
    assert_eq!(indexed.rows, [0x80, 0x80], "9 pixels of 1 bit take 2 bytes; the rest is cut");
    assert_eq!(indexed.palette.as_deref(), Some(&palette[..]));
}

#[test]
fn wraps_fax_data_in_a_tiff() {
    // Fax coding, BlackIs1, EncodedByteAlign, then the tags that say them.
    const CASES: &[(Fax, bool, bool, Tags)] = &[
        (Fax::Group4, true, false, [(259, 4), (262, 0), (293, 0)]),
        (Fax::Group3OneDimensional, false, true, [(259, 3), (262, 1), (292, 4)]),
        (Fax::Group3TwoDimensional, false, false, [(259, 3), (262, 1), (292, 1)]),
    ];
    for &(fax, black_is_one, aligned, expected) in CASES {
        let data = tiff(1728, 2, fax, black_is_one, aligned, b"fax!");
        let (tags, strip) = tiff_of(&data);
        assert_eq!(strip, b"fax!", "{fax:?}");
        assert_eq!(
            [tags[&256], tags[&257], tags[&258], tags[&277], tags[&278]],
            [1728, 2, 1, 1, 2]
        );
        for (tag, value) in expected {
            assert_eq!(tags.get(&tag), Some(&value), "{fax:?}: tag {tag}");
        }
    }
}
