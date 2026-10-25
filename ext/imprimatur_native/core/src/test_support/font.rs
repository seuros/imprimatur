//! Font fixture mutations.

#![cfg(test)]

use read_fonts::{FontRef, types::Tag};

pub fn record(data: &[u8], tag: [u8; 4]) -> usize {
    let font = FontRef::new(data).unwrap();
    let index = font
        .table_directory()
        .table_records()
        .iter()
        .position(|record| record.tag() == Tag::new(&tag))
        .unwrap();
    12 + 16 * index
}

pub fn offset(data: &[u8], tag: [u8; 4]) -> usize {
    let record = record(data, tag);
    u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize
}

pub fn set_u16(data: &mut [u8], tag: [u8; 4], at: usize, value: u16) {
    let start = offset(data, tag) + at;
    data[start..start + 2].copy_from_slice(&value.to_be_bytes());
}

/// Replaces a table, leaving checksums stale for malformed-input tests.
pub fn replace(data: &mut Vec<u8>, tag: [u8; 4], table: &[u8]) {
    let record = record(data, tag);
    data.resize(data.len().next_multiple_of(4), 0);
    let start = u32::try_from(data.len()).unwrap();
    let length = u32::try_from(table.len()).unwrap();
    data[record + 8..record + 12].copy_from_slice(&start.to_be_bytes());
    data[record + 12..record + 16].copy_from_slice(&length.to_be_bytes());
    data.extend_from_slice(table);
}
