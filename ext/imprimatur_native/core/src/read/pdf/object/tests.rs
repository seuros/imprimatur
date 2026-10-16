use std::assert_matches;

use super::*;

#[test]
fn parses_objects_and_references() {
    let mut parser =
        Parser::new(b"<< /Kids [3 0 R 4 0 R] /Count 2 /Name /A#20B /Flag true /Missing >> 5", 0, 8);
    let Object::Dict(dict) = parser.object().unwrap() else { panic!() };
    assert_eq!(dict.get(b"Kids"), Some(&Object::Array(vec![Object::Ref(3, 0), Object::Ref(4, 0)])));
    assert_eq!(dict.get(b"Count").and_then(Object::as_integer), Some(2));
    assert_eq!(dict.get(b"Name").and_then(Object::as_name), Some(&b"A B"[..]));
    assert_eq!(dict.get(b"Flag"), Some(&Object::Bool(true)));
    assert_eq!(dict.get(b"Missing"), None, "a key without a value is dropped");
    assert_eq!(parser.object().unwrap(), Object::Integer(5), "an integer not followed by `G R`");

    let deep = "[".repeat(10);
    assert_matches!(Parser::new(deep.as_bytes(), 0, 8).object(), Err(Error::LimitExceeded(_)));
}

#[test]
fn locates_stream_data() {
    let data = b"7 0 obj << /Length 5 >> stream\r\nhello\nendstream endobj";
    let (number, generation, object) = Parser::new(data, 0, 8).indirect(&direct_length).unwrap();
    let Object::Stream(stream) = object else { panic!() };
    assert_eq!((number, generation, &data[stream.data]), (7, 0, &b"hello"[..]));

    // A wrong /Length: the data runs to endstream.
    let data = b"8 0 obj << /Length 99 >> stream\nhello world\r\nendstream endobj";
    let (_, _, object) = Parser::new(data, 0, 8).indirect(&direct_length).unwrap();
    let Object::Stream(stream) = object else { panic!() };
    assert_eq!(&data[stream.data], b"hello world");

    assert_matches!(
        Parser::new(b"garbage", 0, 8).indirect(&|_| None),
        Err(Error::Malformed { .. })
    );
}
