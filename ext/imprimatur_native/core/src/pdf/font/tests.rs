use std::assert_matches;

use super::*;

#[test]
fn encodes_windows_1252() {
    assert_eq!(Face::Regular.encode("Aé€—").unwrap(), [b'A', 0xE9, 0x80, 0x97]);
    assert_eq!(Face::Bold.encode("a\tb\nc").unwrap(), b"a\tb\nc");
    for text in ["مرحبا", "日本", "Ω", "\u{81}"] {
        assert_matches!(
            Face::Regular.encode(text),
            Err(Error::UnsupportedCharacter { font: "Helvetica", .. }),
            "{text}"
        );
    }
}

#[test]
fn measures_with_adobe_widths() {
    // Helvetica: H 722, e 556, l 222, o 556; bold is wider.
    let hello = Face::Regular.encode("Hello").unwrap();
    assert!((Face::Regular.width(&hello, 10.0) - 22.78).abs() < 1e-9);
    assert!(Face::Bold.width(&hello, 10.0) > Face::Regular.width(&hello, 10.0));
    assert!((Face::Regular.ascent(10.0) - 7.18).abs() < 1e-9);
    assert_eq!(Face::new(true, true), Face::BoldItalic);
}
