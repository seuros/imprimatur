use std::io::Cursor;

use ::zip::CompressionMethod::{Deflated, Stored};

use super::*;
use crate::test_support::read_zip;

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Vec::new());
    for (name, data) in entries {
        zip.add(name, data).unwrap();
    }
    zip.finish().unwrap()
}

#[test]
fn an_independent_reader_reads_every_entry_back() {
    let xml = "<row><c>1</c></row>".repeat(500);
    let entries: &[(&str, &[u8])] = &[
        ("[Content_Types].xml", b"<Types/>"),
        ("xl/empty.xml", b""),
        ("a/b/c.bin", &[0, 1, 2, 255]),
        ("xl/worksheets/sheet1.xml", xml.as_bytes()),
    ];
    let read: Vec<(String, Vec<u8>)> = read_zip(archive(entries));
    let expected: Vec<(String, Vec<u8>)> =
        entries.iter().map(|(name, data)| ((*name).to_owned(), data.to_vec())).collect();
    assert_eq!(read, expected);
}

#[test]
fn deflates_only_what_gets_smaller() {
    let xml = "<row><c>1</c></row>".repeat(500);
    let bytes = archive(&[("big.xml", xml.as_bytes()), ("tiny.xml", b"<a/>"), ("empty", b"")]);
    assert!(bytes.len() < xml.len() / 10, "{} bytes for {} of XML", bytes.len(), xml.len());

    let mut reader = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let methods: Vec<_> =
        (0..reader.len()).map(|index| reader.by_index(index).unwrap().compression()).collect();
    assert_eq!(methods, [Deflated, Stored, Stored]);
}

#[test]
fn output_is_deterministic() {
    let xml = "<row/>".repeat(100);
    let entries: &[(&str, &[u8])] = &[("one.xml", xml.as_bytes()), ("two.xml", b"<two/>")];
    assert_eq!(archive(entries), archive(entries));
}

#[test]
fn an_empty_archive_is_just_the_end_record() {
    let bytes = archive(&[]);
    assert_eq!(bytes.len(), 22);
    assert!(bytes.starts_with(&END_OF_CENTRAL_DIRECTORY.to_le_bytes()));
}
