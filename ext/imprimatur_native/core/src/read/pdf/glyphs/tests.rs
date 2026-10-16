use super::*;

#[test]
fn names_glyphs() {
    const CASES: &[(&str, Option<&str>)] = &[
        ("A", Some("A")),
        ("fi", Some("\u{FB01}")),
        ("quoteright", Some("\u{2019}")),
        ("Euro", Some("€")),
        ("a1", Some("\u{2701}")),
        ("a12", Some("\u{261E}")),
        ("uni0041", Some("A")),
        ("uni00410042", Some("AB")),
        ("uniD800", None),
        ("uni004", None),
        ("u1F600", Some("😀")),
        ("f_f_i", Some("ffi")),
        ("A.sc", Some("A")),
        ("g123", None),
        ("", None),
    ];
    for &(name, text) in CASES {
        assert_eq!(named(name).as_deref(), text, "{name:?}");
    }
}

#[test]
fn encodes_codes_and_decomposes_presentation_forms() {
    assert_eq!(encoded(BaseEncoding::WinAnsi, 0x80), "€");
    assert_eq!(encoded(BaseEncoding::Standard, 0x27), "\u{2019}");
    assert_eq!(encoded(BaseEncoding::MacRoman, 0x8E), "é");
    assert_eq!(encoded(BaseEncoding::Symbol, 0x61), "α");
    assert_eq!(encoded(BaseEncoding::Standard, 0x80), "", "undefined");
    assert_eq!(decomposed('\u{FB01}'), Some("fi"));
    assert_eq!(decomposed('\u{FE8D}'), Some("\u{627}"), "an isolated alef");
    assert_eq!(decomposed('\u{FDF2}'), Some("\u{627}\u{644}\u{644}\u{647}"));
    assert_eq!(decomposed('a'), None);
}
