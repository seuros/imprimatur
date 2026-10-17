//! A `CIDFont`'s TrueType program retains every glyph but omits the cmap table
//! (ISO 32000-1, 9.9). Rebuild the sfnt directory, padding and checksums; glyph
//! outlines, IDs, metrics and hinting tables are unchanged.

#[cfg(test)]
mod tests;

use read_fonts::{FontRef, TableRecord, types::Tag};

use crate::font::TrueTypeFont;
use crate::{Error, Result};

pub fn for_pdf(font: &TrueTypeFont<'_>) -> Result<Vec<u8>> {
    let source = FontRef::new(font.as_bytes()).map_err(invalid)?;
    let mut tags: Vec<_> = source
        .table_directory()
        .table_records()
        .iter()
        .map(TableRecord::tag)
        // Rewriting the directory also invalidates any digital signature.
        .filter(|tag| *tag != Tag::new(b"cmap") && *tag != Tag::new(b"DSIG"))
        .collect();
    tags.sort_unstable();
    (!tags.windows(2).any(|pair| pair[0] == pair[1]))
        .ok_or_else(|| invalid("duplicate sfnt tables"))?;
    let count = u16::try_from(tags.len()).map_err(invalid)?;
    // searchRange and rangeShift are byte counts in 16-bit fields.
    (count > 0 && count <= 4095).ok_or_else(|| invalid("sfnt table count out of range"))?;
    let selector = u16::try_from(count.ilog2()).map_err(invalid)?;
    let range = (1_u16 << selector) * 16;
    let mut data = Vec::with_capacity(font.as_bytes().len());
    data.extend_from_slice(&font.as_bytes()[..4]);
    for value in [count, range, selector, count * 16 - range] {
        data.extend_from_slice(&value.to_be_bytes());
    }
    data.resize(12 + 16 * tags.len(), 0);
    let mut head = None;
    for (index, tag) in tags.into_iter().enumerate() {
        let table = source.table_data(tag).ok_or_else(|| invalid("missing sfnt table"))?;
        let start = data.len();
        let offset = u32::try_from(start).map_err(invalid)?;
        let length = u32::try_from(table.len()).map_err(invalid)?;
        data.extend_from_slice(table.as_bytes());
        if tag == Tag::new(b"head") {
            // head's own checksum is calculated with checkSumAdjustment zero.
            data[start + 8..start + 12].fill(0);
            head = Some(start);
        }
        let sum = checksum(&data[start..]);
        data.resize(data.len().next_multiple_of(4), 0);
        let record = 12 + 16 * index;
        data[record..record + 4].copy_from_slice(&tag.to_be_bytes());
        for (field, value) in [sum, offset, length].into_iter().enumerate() {
            let field = record + 4 * (field + 1);
            data[field..field + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
    let head = head.ok_or_else(|| invalid("missing head table"))?;
    let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&data));
    data[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    Ok(data)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0_u32, |sum, bytes| {
        let mut word = [0; 4];
        word[..bytes.len()].copy_from_slice(bytes);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn invalid(reason: impl std::fmt::Display) -> Error {
    Error::malformed("TrueType font", reason.to_string())
}
