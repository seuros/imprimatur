//! Helpers shared by the unit tests.

#[cfg(feature = "fonts")]
pub mod font;

use std::io::{Cursor, Read};

/// Shared PDF/DOCX round-trip fixture.
pub fn report() -> crate::doc::Document {
    use crate::doc::{Document, Paragraph, Run, Table};

    let mut doc = Document::new();
    doc.heading(1, "Quarterly report").unwrap();
    doc.heading(2, "Sales").unwrap();
    doc.push_paragraph(
        Paragraph::new().text("Revenue was ").run(Run::new("up 12%").bold()).text(" this quarter."),
    )
    .unwrap();
    doc.table(Table::new([["Region", "Q1"], ["North", "12.5"]]).header()).unwrap();
    doc
}

/// An integer entry in a PDF fixture's dictionary.
pub fn pdf_integer(dictionary: &str, name: &str) -> usize {
    dictionary
        .split(&format!("/{name} "))
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// Every entry of a zip archive as `(name, bytes)`, read with the `zip`
/// crate: an implementation independent of ours, which verifies each CRC.
pub fn read_zip(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            (file.name().unwrap().into_owned(), data)
        })
        .collect()
}

/// Every entry of a zip archive as text, by name, read as [`read_zip`] does;
/// media come out garbled, for their names.
pub fn text_parts(bytes: Vec<u8>) -> Vec<(String, String)> {
    read_zip(bytes)
        .into_iter()
        .map(|(name, data)| (name, String::from_utf8_lossy(&data).into_owned()))
        .collect()
}

/// The text of the part `name` among `parts`.
pub fn part<'a>(parts: &'a [(String, String)], name: &str) -> &'a str {
    parts
        .iter()
        .find(|(part, _)| part == name)
        .map_or_else(|| panic!("no part {name}"), |(_, xml)| xml)
}

/// A zip of `(name, text)` parts, written with our own writer.
pub fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = crate::zip::ZipWriter::new(Vec::new());
    for (name, text) in parts {
        zip.add(name, text.as_bytes()).unwrap();
    }
    zip.finish().unwrap()
}

/// A PDF of `objects`, numbered from 1 (the first is the catalog), with a
/// classic cross-reference table.
pub fn pdf_of(objects: &[Vec<u8>]) -> Vec<u8> {
    use std::io::Write as _;
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (number, body) in (1..).zip(objects) {
        offsets.push(out.len());
        writeln!(out, "{number} 0 obj").unwrap();
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).unwrap();
    for offset in offsets {
        writeln!(out, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        out,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    out
}

/// A stream object: `dict` holds the entries besides `/Length`.
pub fn stream_object(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {dict} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// One page of `content` with Helvetica as `/F1` and a 612 by 792 page.
pub fn page_pdf(content: &str, page_extra: &str) -> Vec<u8> {
    pdf_of(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R {page_extra} >>"
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        stream_object("", content.as_bytes()),
    ])
}

/// A PNG as `read::image::png` writes it, read back.
#[derive(Debug)]
pub struct Png {
    pub width: u32,
    pub height: u32,
    pub bits: u8,
    pub color: u8,
    pub palette: Option<Vec<u8>>,
    /// The rows, their filter bytes (always 0, none) taken out.
    pub rows: Vec<u8>,
}

/// Reads a PNG, checking each chunk's CRC and that every row is unfiltered.
pub fn png_of(data: &[u8]) -> Png {
    let be = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap());
    assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"), "{data:?}");
    let (mut header, mut palette, mut compressed) = (Vec::new(), None, Vec::new());
    let mut at = 8;
    while at < data.len() {
        let length = be(at) as usize;
        let (kind, body) = (&data[at + 4..at + 8], &data[at + 8..at + 8 + length]);
        assert_eq!(
            be(at + 8 + length),
            crc32fast::hash(&data[at + 4..at + 8 + length]),
            "{kind:?}"
        );
        match kind {
            b"IHDR" => header = body.to_vec(),
            b"PLTE" => palette = Some(body.to_vec()),
            b"IDAT" => compressed.extend_from_slice(body),
            _ => {}
        }
        at += 12 + length;
    }
    let (width, height) = (be(16), be(20));
    let (bits, color) = (header[8], header[9]);
    let channels = match color {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    };
    let stride = (width as usize * channels * usize::from(bits)).div_ceil(8);
    let filtered = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed).unwrap();
    assert_eq!(filtered.len(), (stride + 1) * height as usize);
    let mut rows = Vec::new();
    for row in filtered.chunks(stride + 1) {
        assert_eq!(row[0], 0, "filter type");
        rows.extend_from_slice(&row[1..]);
    }
    Png { width, height, bits, color, palette, rows }
}

/// A one-strip little-endian TIFF's tags, by number, and its strip.
pub fn tiff_of(data: &[u8]) -> (std::collections::BTreeMap<u16, u32>, &[u8]) {
    let le16 = |at: usize| u16::from_le_bytes(data[at..at + 2].try_into().unwrap());
    let le32 = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    assert!(data.starts_with(b"II*\0"));
    let directory = le32(4) as usize;
    let tags: std::collections::BTreeMap<u16, u32> = (0..usize::from(le16(directory)))
        .map(|index| {
            let entry = directory + 2 + index * 12;
            let value =
                if le16(entry + 2) == 3 { u32::from(le16(entry + 8)) } else { le32(entry + 8) };
            (le16(entry), value)
        })
        .collect();
    let (offset, length) = (tags[&273] as usize, tags[&279] as usize);
    (tags, &data[offset..offset + length])
}

/// A compound file (MS-CFB, version 3) holding one stream named `name` in
/// its root storage; a stream under 4096 bytes goes to the mini stream, as
/// writers put it.
pub fn ole_of(name: &str, stream: &[u8]) -> Vec<u8> {
    const SECTOR: usize = 512;
    const END: u32 = 0xFFFF_FFFE;
    let small = stream.len() < 4096;
    let pad = |data: &[u8], size: usize| {
        let mut data = data.to_vec();
        data.resize(data.len().div_ceil(size).max(1) * size, 0);
        data
    };
    // Sectors: FAT, directory, mini FAT (when small), then the data: the
    // stream itself, or the mini stream holding it.
    let data = if small { pad(stream, 64) } else { stream.to_vec() };
    let data_sectors = data.len().div_ceil(SECTOR).max(1);
    let first_data = if small { 3 } else { 2 };
    let mut fat = vec![0xFFFF_FFFD_u32, END];
    if small {
        fat.push(END);
    }
    fat.extend((0..data_sectors).map(|index| {
        if index + 1 == data_sectors { END } else { u32::try_from(first_data + index + 1).unwrap() }
    }));
    fat.resize(SECTOR / 4, 0xFFFF_FFFF);
    let mut header = vec![0_u8; SECTOR];
    header[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    let put = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    header[0x18..0x1A].copy_from_slice(&0x3E_u16.to_le_bytes());
    header[0x1A..0x1C].copy_from_slice(&3_u16.to_le_bytes());
    header[0x1C..0x1E].copy_from_slice(&0xFFFE_u16.to_le_bytes());
    header[0x1E..0x20].copy_from_slice(&9_u16.to_le_bytes());
    header[0x20..0x22].copy_from_slice(&6_u16.to_le_bytes());
    put(&mut header, 0x2C, 1);
    put(&mut header, 0x30, 1);
    put(&mut header, 0x38, 4096);
    put(&mut header, 0x3C, if small { 2 } else { END });
    put(&mut header, 0x40, u32::from(small));
    put(&mut header, 0x44, END);
    put(&mut header, 0x4C, 0);
    for index in 1..109 {
        put(&mut header, 0x4C + index * 4, 0xFFFF_FFFF);
    }
    let entry = |name: &str, kind: u8, child: u32, start: u32, size: usize| {
        let mut bytes = vec![0_u8; 128];
        let units: Vec<u16> = name.encode_utf16().collect();
        for (index, unit) in units.iter().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[0x40..0x42]
            .copy_from_slice(&u16::try_from(units.len() * 2 + 2).unwrap().to_le_bytes());
        bytes[0x42] = kind;
        for at in [0x44, 0x48, 0x4C] {
            put(&mut bytes, at, 0xFFFF_FFFF);
        }
        put(&mut bytes, 0x4C, child);
        put(&mut bytes, 0x74, start);
        put(&mut bytes, 0x78, u32::try_from(size).unwrap());
        bytes
    };
    let first = u32::try_from(first_data).unwrap();
    let mut directory = if small {
        let mut directory = entry("Root Entry", 5, 1, first, data.len());
        directory.extend(entry(name, 2, 0xFFFF_FFFF, 0, stream.len()));
        directory
    } else {
        let mut directory = entry("Root Entry", 5, 1, END, 0);
        directory.extend(entry(name, 2, 0xFFFF_FFFF, first, stream.len()));
        directory
    };
    directory.resize(SECTOR, 0);
    let mut file = header;
    file.extend(fat.iter().flat_map(|word| word.to_le_bytes()));
    file.extend(directory);
    if small {
        let mut mini_fat: Vec<u32> =
            (1..data.len() / 64).map(|next| u32::try_from(next).unwrap()).collect();
        mini_fat.push(END);
        mini_fat.resize(SECTOR / 4, 0xFFFF_FFFF);
        file.extend(mini_fat.iter().flat_map(|word| word.to_le_bytes()));
    }
    file.extend(pad(&data, SECTOR));
    file
}
