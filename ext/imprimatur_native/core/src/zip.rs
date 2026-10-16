//! A write-only zip archive: enough for the Open Packaging Conventions
//! container that XLSX and DOCX are. Entries are deflated (`miniz_oxide`), or
//! stored when deflate would not make them smaller.
//!
//! Output is deterministic: every entry carries the zip epoch (1980-01-01
//! 00:00) as its timestamp, and deflate runs at a fixed level, so the same
//! document always gives the same bytes.

pub mod reader;
#[cfg(test)]
mod tests;

use std::io::Write;

use crate::{Error, Result};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
/// 2.0: the version that introduced folders, which every reader handles.
const VERSION: u16 = 20;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATED: u16 = 8;
/// zlib's default trade-off between size and speed.
const DEFLATE_LEVEL: u8 = 6;
/// 00:00:00 in MS-DOS time.
const DOS_TIME: u16 = 0;
/// 1980-01-01 in MS-DOS date: day 1, month 1, year 0 (1980).
const DOS_DATE: u16 = (1 << 5) | 1;

struct Entry {
    name: String,
    name_len: u16,
    method: u16,
    crc: u32,
    compressed_size: u32,
    size: u32,
    offset: u32,
}

/// Streams entries into `out`, then the central directory on [`finish`](Self::finish).
pub struct ZipWriter<W: Write> {
    out: W,
    entries: Vec<Entry>,
    written: u64,
}

impl<W: Write> ZipWriter<W> {
    pub const fn new(out: W) -> Self {
        Self { out, entries: Vec::new(), written: 0 }
    }

    /// Writes one entry. Names are ASCII part names such as
    /// `xl/workbook.xml`, so the UTF-8 name flag is never needed.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        debug_assert!(name.is_ascii(), "OPC part names are ASCII: {name:?}");
        let offset = self.offset()?;
        let too_large = |_| Error::ArchiveTooLarge("entry over 4 GiB");
        let size = u32::try_from(data.len()).map_err(too_large)?;
        let name_len =
            u16::try_from(name.len()).map_err(|_| Error::ArchiveTooLarge("entry name"))?;
        let crc = crc32fast::hash(data);

        let deflated = miniz_oxide::deflate::compress_to_vec(data, DEFLATE_LEVEL);
        let (method, body) = if deflated.len() < data.len() {
            (METHOD_DEFLATED, deflated.as_slice())
        } else {
            (METHOD_STORED, data)
        };
        let compressed_size = u32::try_from(body.len()).map_err(too_large)?;

        let mut header = Vec::with_capacity(30 + name.len());
        put32(&mut header, LOCAL_HEADER);
        put16(&mut header, VERSION);
        put16(&mut header, 0); // flags
        put16(&mut header, method);
        put16(&mut header, DOS_TIME);
        put16(&mut header, DOS_DATE);
        put32(&mut header, crc);
        put32(&mut header, compressed_size);
        put32(&mut header, size);
        put16(&mut header, name_len);
        put16(&mut header, 0); // extra field
        header.extend_from_slice(name.as_bytes());

        self.write(&header)?;
        self.write(body)?;
        self.entries.push(Entry {
            name: name.to_owned(),
            name_len,
            method,
            crc,
            compressed_size,
            size,
            offset,
        });
        Ok(())
    }

    /// Writes the central directory and returns the underlying writer.
    pub fn finish(mut self) -> Result<W> {
        let directory_offset = self.offset()?;
        let count = u16::try_from(self.entries.len())
            .map_err(|_| Error::ArchiveTooLarge("more than 65535 entries"))?;

        let mut directory = Vec::new();
        for entry in &self.entries {
            put32(&mut directory, CENTRAL_HEADER);
            put16(&mut directory, VERSION); // made by: MS-DOS, 2.0
            put16(&mut directory, VERSION); // needed to extract
            put16(&mut directory, 0); // flags
            put16(&mut directory, entry.method);
            put16(&mut directory, DOS_TIME);
            put16(&mut directory, DOS_DATE);
            put32(&mut directory, entry.crc);
            put32(&mut directory, entry.compressed_size);
            put32(&mut directory, entry.size);
            put16(&mut directory, entry.name_len);
            put16(&mut directory, 0); // extra field
            put16(&mut directory, 0); // comment
            put16(&mut directory, 0); // disk number
            put16(&mut directory, 0); // internal attributes
            put32(&mut directory, 0); // external attributes
            put32(&mut directory, entry.offset);
            directory.extend_from_slice(entry.name.as_bytes());
        }
        let directory_size = u32::try_from(directory.len())
            .map_err(|_| Error::ArchiveTooLarge("central directory over 4 GiB"))?;

        put32(&mut directory, END_OF_CENTRAL_DIRECTORY);
        put16(&mut directory, 0); // this disk
        put16(&mut directory, 0); // disk with the directory
        put16(&mut directory, count); // entries on this disk
        put16(&mut directory, count); // entries in total
        put32(&mut directory, directory_size);
        put32(&mut directory, directory_offset);
        put16(&mut directory, 0); // comment

        self.write(&directory)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn offset(&self) -> Result<u32> {
        u32::try_from(self.written).map_err(|_| Error::ArchiveTooLarge("archive over 4 GiB"))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

fn put16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn put32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}
