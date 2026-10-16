use std::assert_matches;

use super::*;
use crate::read::pdf::object::Name;

#[test]
fn rc4_matches_the_reference_vectors() {
    assert_eq!(rc4(b"Key", b"Plaintext"), [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]);
    assert_eq!(
        rc4(b"Secret", b"Attack at dawn"),
        [0x45, 0xA0, 0x1F, 0x64, 0x5F, 0xC3, 0x5B, 0x38, 0x35, 0x52, 0x54, 0x4B, 0x9B, 0xF5]
    );
}

#[test]
fn refuses_other_handlers() {
    let dict = Dict(vec![(Name::new(b"Filter"), Object::Name(Name::new(b"Adobe.PubSec")))]);
    assert_matches!(Crypt::new(&dict, b"").err(), Some(Error::UnsupportedDocument(_)));
}
