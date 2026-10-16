use super::*;

#[test]
fn escapes_text_and_attributes() {
    const CASES: &[(&str, &str, &str)] = &[
        ("plain", "plain", "plain"),
        ("a & b", "a &amp; b", "a &amp; b"),
        ("<tag>", "&lt;tag&gt;", "&lt;tag&gt;"),
        ("say \"hi\"", "say \"hi\"", "say &quot;hi&quot;"),
        ("tab\there", "tab\there", "tab&#9;here"),
        ("line\nbreak", "line\nbreak", "line&#10;break"),
        ("cr\r", "cr&#13;", "cr&#13;"),
    ];
    for (input, text, attribute) in CASES {
        let mut buf = String::new();
        escape_into(&mut buf, input, false);
        assert_eq!(buf, *text, "text: {input:?}");
        buf.clear();
        escape_into(&mut buf, input, true);
        assert_eq!(buf, *attribute, "attribute: {input:?}");
    }
}

#[test]
fn writes_nested_elements() {
    let mut xml = XmlWriter::new();
    xml.open("root", &[("xmlns", "urn:x")]);
    xml.element("a", &[("k", "1 < 2")], "x & y");
    xml.empty("b", &[]);
    xml.close("root");
    assert_eq!(
        xml.finish(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <root xmlns=\"urn:x\"><a k=\"1 &lt; 2\">x &amp; y</a><b/></root>"
    );
}

#[test]
fn flags_characters_xml_cannot_carry() {
    for ch in ['\0', '\u{1}', '\u{b}', '\u{c}', '\u{1f}', '\u{fffe}', '\u{ffff}'] {
        assert!(is_forbidden(ch), "{ch:?}");
    }
    for ch in ['\t', '\n', '\r', ' ', 'é', '\u{fffd}', '😀'] {
        assert!(!is_forbidden(ch), "{ch:?}");
    }
}
