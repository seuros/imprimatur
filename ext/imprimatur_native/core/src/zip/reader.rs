//! Reads the zip container of an OOXML package. Entries are inflated on
//! demand, each against the read's decompression budget and checked against
//! its CRC, so a zip bomb fails before it fills memory.

#[cfg(test)]
mod tests;

use super::{
    CENTRAL_HEADER, END_OF_CENTRAL_DIRECTORY, LOCAL_HEADER, METHOD_DEFLATED, METHOD_STORED,
};
use crate::read::Budget;
use crate::{Error, Result};

/// The end of central directory record without its trailing comment.
const END_RECORD: usize = 22;
const MAX_COMMENT: usize = u16::MAX as usize;

struct Entry<'a> {
    name: &'a str,
    method: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    offset: usize,
}

pub struct ZipReader<'a> {
    data: &'a [u8],
    entries: Vec<Entry<'a>>,
}

impl<'a> ZipReader<'a> {
    /// Reads the central directory.
    ///
    /// # Errors
    /// [`Error::Malformed`] when `data` is not a zip archive this reader can
    /// handle (zip64 and encrypted entries are not).
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let search = data.len().saturating_sub(END_RECORD + MAX_COMMENT);
        let end = (search..=data.len().saturating_sub(END_RECORD))
            .rev()
            .find(|&at| u32_at(data, at) == Some(END_OF_CENTRAL_DIRECTORY))
            .ok_or_else(|| malformed("no end of central directory"))?;
        let count = usize::from(u16_at(data, end + 10).ok_or_else(|| malformed("truncated"))?);
        let mut at = u32_at(data, end + 16).ok_or_else(|| malformed("truncated"))? as usize;
        if at == u32::MAX as usize {
            return Err(Error::UnsupportedDocument("zip64 archives".to_owned()));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            (u32_at(data, at) == Some(CENTRAL_HEADER))
                .ok_or_else(|| malformed("bad central directory"))?;
            let field = |offset: usize| u16_at(data, at + offset).map(usize::from);
            let word = |offset: usize| u32_at(data, at + offset);
            let (Some(flags), Some(method), Some(crc), Some(compressed), Some(size)) =
                (field(8), field(10), word(16), word(20), word(24))
            else {
                return Err(malformed("truncated central directory"));
            };
            let (Some(name_len), Some(extra_len), Some(comment_len), Some(offset)) =
                (field(28), field(30), field(32), word(42))
            else {
                return Err(malformed("truncated central directory"));
            };
            if flags & 1 != 0 {
                return Err(Error::UnsupportedDocument("encrypted zip entries".to_owned()));
            }
            let name = data
                .get(at + 46..at + 46 + name_len)
                .and_then(|name| std::str::from_utf8(name).ok())
                .ok_or_else(|| malformed("bad entry name"))?;
            entries.push(Entry {
                name: name.trim_start_matches('/'),
                method: u16::try_from(method).unwrap_or(u16::MAX),
                crc,
                compressed: compressed as usize,
                size: size as usize,
                offset: offset as usize,
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    /// The entries' names, in the archive's order.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.entries.iter().map(|entry| entry.name)
    }

    /// The entry named `name` (OPC part names ignore ASCII case), inflated,
    /// or `None` when the archive has none.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] past the decompression budget, and
    /// [`Error::Malformed`] for a corrupt entry.
    pub fn read(&self, name: &str, budget: &Budget<'_>) -> Result<Option<Vec<u8>>> {
        let name = name.trim_start_matches('/');
        let Some(entry) = self.entries.iter().find(|entry| entry.name.eq_ignore_ascii_case(name))
        else {
            return Ok(None);
        };
        let header = entry.offset;
        (u32_at(self.data, header) == Some(LOCAL_HEADER))
            .ok_or_else(|| malformed("bad local header"))?;
        let (Some(name_len), Some(extra_len)) =
            (u16_at(self.data, header + 26), u16_at(self.data, header + 28))
        else {
            return Err(malformed("truncated local header"));
        };
        let start = header + 30 + usize::from(name_len) + usize::from(extra_len);
        let stored = self
            .data
            .get(start..start + entry.compressed)
            .ok_or_else(|| malformed("entry data past the end"))?;
        (entry.size <= budget.inflatable()).ok_or(Error::LimitExceeded("decompressed size"))?;
        let data = match entry.method {
            METHOD_STORED => stored.to_vec(),
            METHOD_DEFLATED => {
                match crate::read::inflate::inflate(stored, false, entry.size, entry.size) {
                    Ok((data, true)) => data,
                    Ok((_, false)) | Err(_) => return Err(malformed("an entry does not inflate")),
                }
            }
            _ => return Err(Error::UnsupportedDocument("a zip compression method".to_owned())),
        };
        budget.inflate(data.len())?;
        budget.check()?;
        (data.len() == entry.size && crc32fast::hash(&data) == entry.crc)
            .ok_or_else(|| malformed("an entry fails its CRC"))?;
        Ok(Some(data))
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn malformed(reason: &str) -> Error {
    Error::malformed("zip", reason)
}
