use std::assert_matches;

use super::*;

#[test]
fn escapes_what_xml_cannot_carry() {
    const CASES: &[(&str, &str)] = &[
        ("plain", "plain"),
        ("bell\u{7}", "bell_x0007_"),
        ("\0", "_x0000_"),
        ("\u{ffff}", "_xFFFF_"),
        // A literal escape sequence keeps its meaning by escaping the underscore.
        ("_x0041_", "_x005F_x0041_"),
        ("_x00e9_", "_x005F_x00e9_"),
        ("_x00G1_", "_x00G1_"),
        ("_x004_", "_x004_"),
        ("snake_case", "snake_case"),
        ("é_x0041_", "é_x005F_x0041_"),
    ];
    for (input, expected) in CASES {
        assert_eq!(escape(input), *expected, "input: {input:?}");
    }
    assert_matches!(escape("no change"), Cow::Borrowed(_));
    // Linear: a long run of underscores used to rescan the text for each one.
    let underscores = "_".repeat(200_000);
    assert_matches!(escape(&underscores), Cow::Borrowed(_));
}

#[test]
fn preserves_edge_whitespace_only() {
    for (text, preserve) in
        [(" lead", true), ("trail ", true), ("\ttab", true), ("in side", false), ("", false)]
    {
        assert_eq!(needs_preserve(text), preserve, "{text:?}");
    }
}

#[test]
fn numbers_strings_by_first_use() {
    let mut strings = SharedStrings::default();
    assert_eq!(strings.intern("b"), 0);
    assert_eq!(strings.intern("a"), 1);
    assert_eq!(strings.intern("b"), 0);
    assert_eq!(strings.intern(" a"), 2);

    let xml = strings.to_xml();
    assert!(xml.contains(r#"count="4" uniqueCount="3""#), "{xml}");
    assert!(
        xml.contains(
            r#"<si><t>b</t></si><si><t>a</t></si><si><t xml:space="preserve"> a</t></si>"#
        ),
        "{xml}"
    );
}

#[test]
fn unescapes_what_escape_wrote() {
    for text in ["plain", "tab\u{1}x", "_x0041_ literal", "a_b_x", "_x00e9_"] {
        assert_eq!(unescape(&escape(text)), text, "{text:?}");
    }
    assert_eq!(unescape("_x0041_BC"), "ABC");
    assert_eq!(unescape("_xD83D_"), "_xD83D_", "a lone surrogate stays");
    assert_matches!(unescape("no escapes"), Cow::Borrowed(_));
}
