//! PDF file syntax: numbered objects, compressed streams, the
//! cross-reference table and the trailer (ISO 32000-1, section 7).

use std::fmt;
use std::io::Write;

use crate::{Error, Result};

/// The header, with the binary comment that marks the file as binary.
const HEADER: &[u8] = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n";
/// zlib's default trade-off between size and speed.
const LEVEL: u8 = 6;
/// The largest offset a cross-reference entry's ten digits hold.
const MAX_OFFSET: usize = 9_999_999_999;

/// Streams a PDF into `out`; objects are numbered from 1 in the order
/// written, and only their offsets are kept for the cross-reference table.
pub struct PdfWriter<W: Write> {
    out: W,
    written: usize,
    offsets: Vec<usize>,
}

impl<W: Write> PdfWriter<W> {
    pub fn new(out: W) -> Result<Self> {
        let mut writer = Self { out, written: 0, offsets: Vec::new() };
        writer.write(HEADER)?;
        Ok(writer)
    }

    /// Writes `id 0 obj <dictionary> endobj`; ids must come in order.
    pub fn object(&mut self, id: usize, dictionary: fmt::Arguments<'_>) -> Result<()> {
        self.begin(id)?;
        self.write_fmt(format_args!("{dictionary}\nendobj\n"))
    }

    /// Writes a Flate-compressed stream object.
    pub fn stream(&mut self, id: usize, data: &[u8]) -> Result<()> {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(data, LEVEL);
        self.begin(id)?;
        let length = compressed.len();
        self.write_fmt(format_args!("<< /Length {length} /Filter /FlateDecode >>\nstream\n"))?;
        self.write(&compressed)?;
        self.write(b"\nendstream\nendobj\n")
    }

    /// Writes the cross-reference table and the trailer; returns `out`.
    pub fn finish(mut self, root: usize, info: usize) -> Result<W> {
        let xref = self.written;
        let size = self.offsets.len() + 1;
        self.write_fmt(format_args!("xref\n0 {size}\n0000000000 65535 f \n"))?;
        for index in 0..self.offsets.len() {
            let offset = self.offsets[index];
            (offset <= MAX_OFFSET).ok_or(Error::ArchiveTooLarge("PDF over 10 GB"))?;
            self.write_fmt(format_args!("{offset:010} 00000 n \n"))?;
        }
        self.write_fmt(format_args!(
            "trailer\n<< /Size {size} /Root {root} 0 R /Info {info} 0 R >>\n\
             startxref\n{xref}\n%%EOF\n"
        ))?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn begin(&mut self, id: usize) -> Result<()> {
        debug_assert_eq!(id, self.offsets.len() + 1, "PDF objects are written in id order");
        self.offsets.push(self.written);
        self.write_fmt(format_args!("{id} 0 obj\n"))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len();
        Ok(())
    }

    /// Formats straight into `out`, counting what it writes.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        let mut counted = Counted { out: &mut self.out, written: 0 };
        counted.write_fmt(args)?;
        self.written += counted.written;
        Ok(())
    }
}

/// A writer that counts the bytes passing through it.
struct Counted<'a, W: Write> {
    out: &'a mut W,
    written: usize,
}

impl<W: Write> Write for Counted<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.out.write(bytes)?;
        self.written += written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// A number for a content stream: two decimals at most, no trailing zeros,
/// formatted without an intermediate `String`.
#[derive(Clone, Copy)]
pub struct Num(pub f64);

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "page coordinates are far inside i64 hundredths"
        )]
        let hundredths = (self.0 * 100.0).round() as i64;
        if hundredths < 0 {
            f.write_str("-")?;
        }
        let (whole, fraction) = (hundredths.unsigned_abs() / 100, hundredths.unsigned_abs() % 100);
        match fraction {
            0 => write!(f, "{whole}"),
            tenths if tenths % 10 == 0 => write!(f, "{whole}.{}", tenths / 10),
            fraction => write!(f, "{whole}.{fraction:02}"),
        }
    }
}

/// Appends a literal string `( ... )`, escaped.
pub fn push_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.reserve(bytes.len() + 2);
    out.push(b'(');
    for &byte in bytes {
        match byte {
            b'(' | b')' | b'\\' => out.extend_from_slice(&[b'\\', byte]),
            // A bare CR inside a string reads back as LF.
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => out.push(byte),
        }
    }
    out.push(b')');
}
