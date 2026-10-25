use std::io::{Cursor, ErrorKind};

use super::*;
use crate::test_support::pdf_integer;
use std::assert_matches;

#[test]
fn stream_entries_preserve_binary_data_and_object_offsets() {
    let data = b"\0\x01\0\0font bytes\xff\nendstream\n".repeat(40);
    let mut writer = PdfWriter::new(Vec::new()).unwrap();
    writer.object(1, format_args!("<< /Type /Catalog /Pages 2 0 R >>")).unwrap();
    writer.object(2, format_args!("<< /Type /Pages /Kids [3 0 R] /Count 1 >>")).unwrap();
    writer
        .object(
            3,
            format_args!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>"),
        )
        .unwrap();
    writer.stream(4, b"", &[]).unwrap();
    writer.stream(5, &data, &[("Length1", &data.len()), ("Subtype", &"/OpenType")]).unwrap();
    writer.object(6, format_args!("<< /Producer (test) >>")).unwrap();
    let pdf = writer.finish(1, 6).unwrap();

    let start = find(&pdf, b"5 0 obj\n");
    let body = start + find(&pdf[start..], b"stream\n") + b"stream\n".len();
    let dictionary = std::str::from_utf8(&pdf[start..body]).unwrap();
    assert!(dictionary.contains(&format!("/Length1 {} /Subtype /OpenType", data.len())));
    assert!(dictionary.contains("/Filter /FlateDecode"));
    let length = pdf_integer(dictionary, "Length");
    assert_ne!(length, data.len(), "Length counts encoded bytes; Length1 counts decoded bytes");
    assert_eq!(
        miniz_oxide::inflate::decompress_to_vec_zlib(&pdf[body..body + length]).unwrap(),
        data
    );
    assert!(pdf[body + length..].starts_with(b"\nendstream\nendobj\n6 0 obj\n"));

    let xref = find(&pdf, b"xref\n");
    let table = std::str::from_utf8(&pdf[xref..]).unwrap();
    assert!(table.starts_with("xref\n0 7\n"));
    for (id, entry) in (1..).zip(table.lines().skip(3).take(6)) {
        let offset: usize = entry[..10].parse().unwrap();
        assert!(pdf[offset..].starts_with(format!("{id} 0 obj\n").as_bytes()));
    }
    assert!(table.ends_with(&format!("startxref\n{xref}\n%%EOF\n")));
}

#[test]
fn propagates_errors_while_writing_stream_entries() {
    // Enough for the normal stream header, but not the additional dictionary.
    let mut output = [0; 100];
    let mut writer = PdfWriter::new(Cursor::new(output.as_mut_slice())).unwrap();
    let value = "x".repeat(200);
    let result = writer.stream(1, b"data", &[("Example", &value)]);
    assert_matches!(result, Err(Error::Io(error)) if error.kind() == ErrorKind::WriteZero);
}

fn find(data: &[u8], needle: &[u8]) -> usize {
    data.windows(needle.len()).position(|window| window == needle).unwrap()
}
