//! Compound files (MS-CFB): the container legacy Office documents are, a
//! small FAT file system inside one file. Reading a stream of the root
//! storage by name is all the readers need.

#[cfg(test)]
mod tests;

use crate::bytes::{u16_le as u16_at, u32_le as u32_at};
use crate::read::Budget;
use crate::{Error, Result};

/// The first eight bytes of every compound file.
pub const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Sector numbers above this are markers: end of chain, free, FAT, DIFAT.
const MAX_SECTOR: u32 = 0xFFFF_FFFA;
/// No sibling or child in the directory tree.
const NO_STREAM: u32 = 0xFFFF_FFFF;
const MINI_SECTOR: usize = 64;
const ENTRY: usize = 128;

/// A compound file's allocation tables and the root storage's entries.
pub struct Ole<'a> {
    data: &'a [u8],
    sector: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
    mini_cutoff: u64,
    /// The root storage's children: name, whether a stream, start, size.
    entries: Vec<(String, bool, u32, u64)>,
}

fn malformed(reason: &str) -> Error {
    Error::malformed("compound file", reason)
}

/// An allocation table's sector numbers.
fn words(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes.as_chunks::<4>().0.iter().map(|word| u32::from_le_bytes(*word))
}

impl<'a> Ole<'a> {
    /// Reads the header, the allocation tables and the directory.
    ///
    /// # Errors
    /// [`Error::Malformed`] for a file that is not a compound file or whose
    /// tables point outside it or in circles.
    pub fn open(data: &'a [u8], budget: &Budget<'_>) -> Result<Self> {
        if !data.starts_with(&SIGNATURE) || data.len() < 512 {
            return Err(malformed("no compound file header"));
        }
        let header = |at| u32_at(data, at).ok_or_else(|| malformed("a short header"));
        let sector = match u16_at(data, 0x1E) {
            Some(9) => 512,
            Some(12) => 4096,
            _ => return Err(malformed("an unknown sector size")),
        };
        let mut ole = Self {
            data,
            sector,
            fat: Vec::new(),
            mini_fat: Vec::new(),
            mini_stream: Vec::new(),
            mini_cutoff: u64::from(header(0x38)?),
            entries: Vec::new(),
        };
        // The FAT's own sectors: 109 listed in the header, the rest in a
        // chain of DIFAT sectors, each ending with the next one's number.
        let mut fat_sectors: Vec<u32> = (0..109)
            .filter_map(|index| u32_at(data, 0x4C + index * 4))
            .filter(|&s| s <= MAX_SECTOR)
            .collect();
        let mut next = header(0x44)?;
        let per_sector = sector / 4;
        for _ in 0..=data.len() / sector {
            if next > MAX_SECTOR {
                break;
            }
            let bytes = ole.sector_bytes(next)?;
            fat_sectors.extend(
                (0..per_sector - 1)
                    .filter_map(|index| u32_at(bytes, index * 4))
                    .filter(|&s| s <= MAX_SECTOR),
            );
            next = u32_at(bytes, (per_sector - 1) * 4).unwrap_or(NO_STREAM);
        }
        budget.inflate(fat_sectors.len().saturating_mul(sector))?;
        for number in fat_sectors {
            budget.tick()?;
            let bytes = ole.sector_bytes(number)?;
            ole.fat.extend(words(bytes));
        }
        let directory = ole.chain(header(0x30)?, None, budget)?;
        let first_mini_fat = header(0x3C)?;
        if first_mini_fat <= MAX_SECTOR {
            let table = ole.chain(first_mini_fat, None, budget)?;
            ole.mini_fat = words(&table).collect();
        }
        let root = directory.get(..ENTRY).ok_or_else(|| malformed("no root entry"))?;
        if root[0x42] != 5 {
            return Err(malformed("the first entry is not the root"));
        }
        // The whole chain: some writers give the root the size of the one
        // stream in it, short of the last mini sector it fills.
        let (start, _) = location(root, sector);
        ole.mini_stream =
            if start <= MAX_SECTOR { ole.chain(start, None, budget)? } else { Vec::new() };
        ole.entries = children(&directory, sector, budget)?;
        Ok(ole)
    }

    /// The bytes of the stream `name` in the root storage, its name
    /// compared regardless of case; `None` when there is none.
    ///
    /// # Errors
    /// [`Error::Malformed`] for a stream whose sectors are missing or loop.
    pub fn stream(&self, name: &str, budget: &Budget<'_>) -> Result<Option<Vec<u8>>> {
        let Some(&(_, _, start, size)) = self
            .entries
            .iter()
            .find(|(entry, stream, ..)| *stream && entry.eq_ignore_ascii_case(name))
        else {
            return Ok(None);
        };
        if size < self.mini_cutoff {
            return self.mini_chain(start, size, budget).map(Some);
        }
        self.chain(start, Some(size), budget).map(Some)
    }

    fn sector_bytes(&self, number: u32) -> Result<&'a [u8]> {
        let start = (number as usize + 1)
            .checked_mul(self.sector)
            .ok_or_else(|| malformed("a sector out of range"))?;
        self.data
            .get(start..start + self.sector)
            .ok_or_else(|| malformed("a sector past the end of the file"))
    }

    /// The sectors chained from `start`, cut to `size` when given.
    fn chain(&self, start: u32, size: Option<u64>, budget: &Budget<'_>) -> Result<Vec<u8>> {
        let limit = size.map_or(usize::MAX, |size| usize::try_from(size).unwrap_or(usize::MAX));
        if limit != usize::MAX && limit > self.data.len() {
            return Err(malformed("a stream larger than the file"));
        }
        let mut out = Vec::with_capacity(limit.min(self.data.len()));
        let mut seen = vec![false; self.fat.len()];
        let mut next = start;
        while next <= MAX_SECTOR && out.len() < limit {
            budget.tick()?;
            let visited =
                seen.get_mut(next as usize).ok_or_else(|| malformed("a sector beyond the FAT"))?;
            if std::mem::replace(visited, true) {
                return Err(malformed("a sector chain that loops"));
            }
            out.extend_from_slice(self.sector_bytes(next)?);
            next = self.fat[next as usize];
        }
        out.truncate(limit);
        budget.inflate(out.len())?;
        Ok(out)
    }

    /// A small stream, kept in the root's mini stream in 64-byte sectors.
    fn mini_chain(&self, start: u32, size: u64, budget: &Budget<'_>) -> Result<Vec<u8>> {
        let limit = usize::try_from(size).unwrap_or(usize::MAX);
        let mut out = Vec::with_capacity(limit.min(self.mini_stream.len()));
        let mut seen = vec![false; self.mini_fat.len()];
        let mut next = start;
        while next <= MAX_SECTOR && out.len() < limit {
            budget.tick()?;
            let visited = seen
                .get_mut(next as usize)
                .ok_or_else(|| malformed("a sector beyond the mini FAT"))?;
            if std::mem::replace(visited, true) {
                return Err(malformed("a mini sector chain that loops"));
            }
            let at = next as usize * MINI_SECTOR;
            let bytes = self
                .mini_stream
                .get(at..at + MINI_SECTOR)
                .ok_or_else(|| malformed("a mini sector past the mini stream"))?;
            out.extend_from_slice(bytes);
            next = self.mini_fat[next as usize];
        }
        out.truncate(limit);
        budget.inflate(out.len())?;
        Ok(out)
    }
}

/// An entry's first sector and size; files of 512-byte sectors (version 3)
/// leave the size's high half undefined.
fn location(entry: &[u8], sector: usize) -> (u32, u64) {
    let start = u32_at(entry, 0x74).unwrap_or(NO_STREAM);
    let low = u64::from(u32_at(entry, 0x78).unwrap_or(0));
    let high = if sector == 512 { 0 } else { u64::from(u32_at(entry, 0x7C).unwrap_or(0)) };
    (start, high << 32 | low)
}

/// The root storage's children, from the red-black tree under its child
/// pointer: name, whether a stream, first sector, size.
fn children(
    directory: &[u8],
    sector: usize,
    budget: &Budget<'_>,
) -> Result<Vec<(String, bool, u32, u64)>> {
    let entry = |index: u32| directory.get(index as usize * ENTRY..(index as usize + 1) * ENTRY);
    let mut found = Vec::new();
    let mut pending = vec![u32_at(directory, 0x4C).unwrap_or(NO_STREAM)];
    let mut visits = 0_usize;
    while let Some(index) = pending.pop() {
        if index == NO_STREAM {
            continue;
        }
        visits += 1;
        if visits > directory.len() / ENTRY {
            return Err(malformed("a directory tree that loops"));
        }
        budget.tick()?;
        let Some(bytes) = entry(index) else {
            return Err(malformed("a directory entry past the end"));
        };
        let length = usize::from(u16_at(bytes, 0x40).unwrap_or(0)).min(64).saturating_sub(2);
        let units: Vec<u16> = bytes[..length]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|unit| u16::from_le_bytes(*unit))
            .collect();
        let (start, size) = location(bytes, sector);
        found.push((String::from_utf16_lossy(&units), bytes[0x42] == 2, start, size));
        pending.push(u32_at(bytes, 0x44).unwrap_or(NO_STREAM));
        pending.push(u32_at(bytes, 0x48).unwrap_or(NO_STREAM));
    }
    Ok(found)
}
