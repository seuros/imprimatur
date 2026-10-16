use std::assert_matches;

use super::*;

fn events(text: &str) -> Vec<Event<'_>> {
    let mut reader = XmlReader::new(text, 16);
    std::iter::from_fn(|| reader.next().unwrap()).collect()
}

#[test]
fn reads_elements_text_and_attributes() {
    let xml = concat!(
        r#"<?xml version="1.0"?><!-- c --><w:p a="1" w:val='x &amp; y'>"#,
        "Fish &amp; chips<w:br/><![CDATA[<raw>]]></w:p>",
    );
    let events = events(xml);
    let Event::Start(tag) = &events[0] else { panic!("{events:?}") };
    assert_eq!((tag.name(), tag.attr("val").as_deref()), ("p", Some("x & y")));
    assert_eq!(tag.attr("a").as_deref(), Some("1"));
    assert_eq!(tag.attr("missing"), None);
    assert_eq!(events[1], Event::Text(Cow::Borrowed("Fish & chips")));
    assert_matches!(&events[2], Event::Start(tag) if tag.name() == "br");
    assert_eq!(events[3], Event::End("br"), "a self-closing tag ends itself");
    assert_eq!(events[4], Event::Text(Cow::Borrowed("<raw>")));
    assert_eq!(events[5], Event::End("p"));
    assert_eq!(events.len(), 6);
}

#[test]
fn reads_flags_and_references() {
    let flags = r#"<b/><b w:val="0"/><b w:val="true"/><b w:val="off"/>"#;
    let values: Vec<bool> = events(flags)
        .iter()
        .filter_map(|event| match event {
            Event::Start(tag) => Some(tag.flag("val")),
            _ => None,
        })
        .collect();
    assert_eq!(values, [true, false, true, false]);
    assert_eq!(unescape("&#233;&#x41;&lt;&gt;&quot;&apos;").unwrap(), "éA<>\"'");
    assert_matches!(unescape("&nbsp;"), Err(Error::Malformed { .. }));
    assert_matches!(unescape("a & b"), Err(Error::Malformed { .. }));
}

#[test]
fn refuses_dtds_and_limits_depth() {
    let mut dtd = XmlReader::new("<!DOCTYPE x [<!ENTITY a 'b'>]><x/>", 16);
    assert_matches!(dtd.next(), Err(Error::Malformed { .. }));
    let deep = "<a>".repeat(17);
    let mut reader = XmlReader::new(&deep, 16);
    let result = std::iter::from_fn(|| Some(reader.next())).find(Result::is_err);
    assert_matches!(result, Some(Err(Error::LimitExceeded(_))));
    let mut unterminated = XmlReader::new("<a b=\"1", 16);
    assert_matches!(unterminated.next(), Err(Error::Malformed { .. }));
}

#[test]
fn skips_an_element() {
    let mut reader = XmlReader::new("<a><skip><x>text</x><y/></skip><keep/></a>", 16);
    reader.next().unwrap();
    reader.next().unwrap();
    reader.skip().unwrap();
    assert_matches!(reader.next().unwrap(), Some(Event::Start(tag)) if tag.name() == "keep");
}

#[test]
fn decodes_byte_order_marks() {
    let utf16: Vec<u8> =
        [0xFF, 0xFE].into_iter().chain("<a/>".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    assert_eq!(into_text(utf16).unwrap(), "<a/>");
    assert_eq!(into_text(b"\xEF\xBB\xBF<a/>".to_vec()).unwrap(), "<a/>");
    assert_matches!(into_text(vec![0xC3]), Err(Error::Malformed { .. }));
}
