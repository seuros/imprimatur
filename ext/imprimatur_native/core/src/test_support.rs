//! Helpers shared by the unit tests.

use std::io::{Cursor, Read};

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

/// Every entry of a zip archive as text, by name, read as [`read_zip`] does.
pub fn text_parts(bytes: Vec<u8>) -> Vec<(String, String)> {
    read_zip(bytes)
        .into_iter()
        .map(|(name, data)| (name, String::from_utf8(data).unwrap()))
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
