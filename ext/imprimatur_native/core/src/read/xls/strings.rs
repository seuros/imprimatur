//! Workbook strings: BIFF8's Latin-1 or UTF-16, BIFF5's Windows-1252
//! bytes, and BIFF8's shared string table, whose strings may be split
//! across `CONTINUE` records.

use crate::Result;
use crate::bytes::u16_le;
use crate::pdf::metrics::WIN_ANSI_HIGH;
use crate::read::Budget;

/// How a workbook's strings are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// BIFF8: each string says whether it is Latin-1 or UTF-16.
    Unicode,
    /// BIFF5: bytes in the workbook's code page.
    Windows1252,
}

/// A string with a 1-byte or 2-byte character count at `at`; `None` past
/// the end.
pub fn string(bytes: &[u8], at: usize, wide_count: bool, encoding: Encoding) -> Option<String> {
    let (count, at) = if wide_count {
        (usize::from(u16_le(bytes, at)?), at + 2)
    } else {
        (usize::from(*bytes.get(at)?), at + 1)
    };
    if encoding == Encoding::Windows1252 {
        let chars = bytes.get(at..at + count).or_else(|| bytes.get(at..))?;
        return Some(chars.iter().map(|&byte| windows_1252(byte)).collect());
    }
    let high = bytes.get(at)? & 1 != 0;
    let mut out = String::with_capacity(count);
    let mut units = Vec::new();
    let width = if high { 2 } else { 1 };
    let chars = bytes.get(at + 1..at + 1 + count * width).or_else(|| bytes.get(at + 1..))?;
    decode(chars, high, &mut units, &mut out);
    flush(&mut units, &mut out);
    Some(out)
}

/// A Windows-1252 byte: Latin-1, but for the typographic characters at
/// 0x80 to 0x9F.
fn windows_1252(byte: u8) -> char {
    match byte {
        0x80..=0x9F => WIN_ANSI_HIGH[usize::from(byte - 0x80)].unwrap_or_else(|| char::from(byte)),
        _ => char::from(byte),
    }
}

/// Appends characters: Latin-1 bytes at once, UTF-16 units to `units`
/// (a surrogate pair may span two pieces), decoded by [`flush`].
fn decode(chars: &[u8], high: bool, units: &mut Vec<u16>, out: &mut String) {
    if high {
        units.extend(chars.as_chunks::<2>().0.iter().map(|unit| u16::from_le_bytes(*unit)));
    } else {
        flush(units, out);
        out.extend(chars.iter().map(|&byte| char::from(byte)));
    }
}

fn flush(units: &mut Vec<u16>, out: &mut String) {
    out.extend(
        char::decode_utf16(units.drain(..)).map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER)),
    );
}

/// The shared string table (`SST`): its record's body, then each
/// `CONTINUE` body after it.
pub fn shared(parts: &[&[u8]], budget: &Budget<'_>) -> Result<Vec<String>> {
    let mut reader = Cursor { parts, part: 0, at: 8 };
    let Some(unique) = parts.first().and_then(|first| first.get(4..8)) else {
        return Ok(Vec::new());
    };
    let unique = u32::from_le_bytes(unique.try_into().unwrap_or([0; 4])) as usize;
    let mut strings = Vec::with_capacity(unique.min(1 << 20));
    let mut units = Vec::new();
    while strings.len() < unique {
        budget.tick()?;
        let Some(string) = reader.string(&mut units) else { break };
        strings.push(string);
    }
    Ok(strings)
}

/// A cursor over record bodies read as one stream.
struct Cursor<'p, 'a> {
    parts: &'p [&'a [u8]],
    part: usize,
    at: usize,
}

impl Cursor<'_, '_> {
    fn byte(&mut self) -> Option<u8> {
        while self.at >= self.parts.get(self.part)?.len() {
            self.part += 1;
            self.at = 0;
        }
        let byte = self.parts[self.part][self.at];
        self.at += 1;
        Some(byte)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.byte()?, self.byte()?]))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes([self.byte()?, self.byte()?, self.byte()?, self.byte()?]))
    }

    fn skip(&mut self, mut count: usize) -> Option<()> {
        while count > 0 {
            let left = self.parts.get(self.part)?.len().saturating_sub(self.at);
            if left == 0 {
                self.part += 1;
                self.at = 0;
                continue;
            }
            let step = left.min(count);
            self.at += step;
            count -= step;
        }
        Some(())
    }

    /// An `XLUnicodeRichExtendedString`: where its characters cross into the
    /// next record, that record starts with their width again.
    fn string(&mut self, units: &mut Vec<u16>) -> Option<String> {
        let count = usize::from(self.u16()?);
        let flags = self.byte()?;
        let runs = if flags & 0x08 != 0 { usize::from(self.u16()?) } else { 0 };
        let extended = if flags & 0x04 != 0 { self.u32()? as usize } else { 0 };
        let mut high = flags & 1 != 0;
        let mut out = String::with_capacity(count);
        let mut left = count;
        while left > 0 {
            let part = self.parts.get(self.part)?;
            if self.at >= part.len() {
                self.part += 1;
                self.at = 1;
                high = self.parts.get(self.part)?.first()? & 1 != 0;
                continue;
            }
            let width = if high { 2 } else { 1 };
            let take = ((part.len() - self.at) / width).min(left);
            if take == 0 {
                return None;
            }
            decode(&part[self.at..self.at + take * width], high, units, &mut out);
            self.at += take * width;
            left -= take;
        }
        flush(units, &mut out);
        self.skip(runs * 4 + extended)?;
        Some(out)
    }
}
