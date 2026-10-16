use std::assert_matches;

use super::*;
use crate::read::Limits;
use crate::read::pdf::object::Name;

fn decoded(data: &[u8], filter: &[u8], params: Option<&Dict>) -> Vec<u8> {
    let limits = Limits::default();
    let budget = Budget::new(&limits, data.len()).unwrap();
    decode(data, &[(filter, params)], &budget).unwrap()
}

#[test]
fn inflates_even_damaged_streams() {
    let text: Vec<u8> =
        (0..2_000).flat_map(|line| format!("BT ({line}) Tj ET\n").into_bytes()).collect();
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&text, 6);
    assert_eq!(decoded(&zlib, b"FlateDecode", None), text);
    let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
    assert_eq!(decoded(&raw, b"Fl", None), text, "no zlib header");
    let truncated = &zlib[..zlib.len() / 2];
    let partial = decoded(truncated, b"FlateDecode", None);
    assert!(
        partial.len() > 1_000 && text.starts_with(&partial),
        "what inflated before the damage: {}",
        partial.len()
    );

    let limits = Limits::default().max_inflated(1_000);
    let budget = Budget::new(&limits, zlib.len()).unwrap();
    assert_matches!(
        decode(&zlib, &[(b"FlateDecode", None)], &budget),
        Err(Error::LimitExceeded(_))
    );
}

#[test]
fn undoes_png_predictors() {
    let params = Dict(vec![
        (Name::new(b"Predictor"), Object::Integer(12)),
        (Name::new(b"Columns"), Object::Integer(3)),
    ]);
    // Rows: none [1 2 3], up [+1 +1 +1].
    let rows = [0, 1, 2, 3, 2, 1, 1, 1];
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&rows, 6);
    assert_eq!(decoded(&zlib, b"FlateDecode", Some(&params)), [1, 2, 3, 2, 3, 4]);
}

#[test]
fn decodes_the_other_filters() {
    // ISO 32000-1, 7.4.4.2's example.
    assert_eq!(
        decoded(&[0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01], b"LZWDecode", None),
        b"-----A---B"
    );
    assert_eq!(decoded(b"87cURD]i,\"Ebo80~>", b"ASCII85Decode", None), b"Hello World!");
    assert_eq!(decoded(b"z~>", b"A85", None), [0, 0, 0, 0]);
    assert_eq!(decoded(b"48 65 6c 6c 6f>", b"ASCIIHexDecode", None), b"Hello");
    assert_eq!(
        decoded(&[2, b'a', b'b', b'c', 254, b'x', 128], b"RunLengthDecode", None),
        b"abcxxx"
    );
    assert_eq!(decoded(b"data", b"Crypt", None), b"data");
}
