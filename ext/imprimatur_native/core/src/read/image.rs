//! Images found in documents, as files: what PDFs store as pixels becomes
//! PNG (or TIFF, for fax-compressed scans); JPEG and JPEG 2000 are kept as
//! they are; Office documents' media are copied.

mod dimensions;
#[cfg(test)]
mod tests;

use super::content::Bounds;

/// An image, encoded as a file of its format.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub data: Vec<u8>,
    pub format: ImageFormat,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub location: ImageLocation,
}

/// Where an image was found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ImageLocation {
    /// On a PDF page (the first that draws it), placed at `bounds`.
    Page { number: u32, bounds: Option<Bounds> },
    /// A part of a Word or Excel package, and for a workbook the sheet that
    /// shows it.
    Part { name: String, sheet: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageFormat {
    Png,
    Jpeg,
    Jpeg2000,
    Tiff,
    Gif,
    Bmp,
    Webp,
    Svg,
    Emf,
    Wmf,
    /// A PDF's JBIG2 stream, globals first: viewers do not open it, a JBIG2
    /// decoder in embedded mode does.
    Jbig2,
    Other,
}

impl ImageFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Jpeg2000 => "jp2",
            Self::Tiff => "tif",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Webp => "webp",
            Self::Svg => "svg",
            Self::Emf => "emf",
            Self::Wmf => "wmf",
            Self::Jbig2 => "jb2",
            Self::Other => "bin",
        }
    }

    /// The format of a file, by its first bytes, or by its name's
    /// extension when they say nothing.
    pub(crate) fn sniff(data: &[u8], name: &str) -> Self {
        match data {
            [0x89, b'P', b'N', b'G', ..] => Self::Png,
            [0xFF, 0xD8, ..] => Self::Jpeg,
            [b'G', b'I', b'F', b'8', ..] => Self::Gif,
            [b'B', b'M', ..] => Self::Bmp,
            [b'I', b'I', 42, 0, ..] | [b'M', b'M', 0, 42, ..] => Self::Tiff,
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Self::Webp,
            [0xD7, 0xCD, 0xC6, 0x9A, ..] => Self::Wmf,
            [0, 0, 0, 0x0C, b'j', b'P', ..] | [0xFF, 0x4F, 0xFF, 0x51, ..] => Self::Jpeg2000,
            _ if data.get(40..44) == Some(b" EMF") => Self::Emf,
            _ if is_svg(data) => Self::Svg,
            _ => name.rsplit_once('.').map_or(Self::Other, |(_, extension)| {
                match extension.to_ascii_lowercase().as_str() {
                    "svg" => Self::Svg,
                    "emf" => Self::Emf,
                    "wmf" => Self::Wmf,
                    _ => Self::Other,
                }
            }),
        }
    }

    /// Width and height, read from the file's header where the format puts
    /// them up front.
    pub(crate) fn dimensions(self, data: &[u8]) -> Option<(u32, u32)> {
        dimensions::dimensions(self, data)
    }
}

/// Whether a file is SVG markup: an `<svg` element among its first bytes,
/// after an XML declaration, comments or a doctype at most.
fn is_svg(data: &[u8]) -> bool {
    let head = &data[..data.len().min(1024)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head).trim_ascii_start();
    head.starts_with(b"<")
        && !head.starts_with(b"<html")
        && head.windows(4).any(|window| window == b"<svg")
}

/// PNG color types (ISO/IEC 15948, 11.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngColor {
    Gray = 0,
    Rgb = 2,
    Palette = 3,
    GrayAlpha = 4,
    Rgba = 6,
}

/// A PNG of `rows` packed pixels, `bits` per sample, each row starting on a
/// byte. `palette` is RGB triples, for [`PngColor::Palette`].
pub fn png(
    width: u32,
    height: u32,
    color: PngColor,
    bits: u8,
    palette: Option<&[u8]>,
    rows: &[u8],
) -> Vec<u8> {
    let channels: usize = match color {
        PngColor::Gray | PngColor::Palette => 1,
        PngColor::GrayAlpha => 2,
        PngColor::Rgb => 3,
        PngColor::Rgba => 4,
    };
    let stride = (width as usize * channels * usize::from(bits)).div_ceil(8);
    let mut filtered = Vec::with_capacity((stride + 1) * height as usize);
    // Short data, as damaged files have, is padded with zero rows.
    for row in 0..height as usize {
        filtered.push(0);
        let line = rows.get(row * stride..).unwrap_or_default();
        let line = &line[..line.len().min(stride)];
        filtered.extend_from_slice(line);
        filtered.resize(filtered.len() + stride - line.len(), 0);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&filtered, 6);
    let mut out = Vec::with_capacity(compressed.len() + 128);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = [0_u8; 13];
    header[..4].copy_from_slice(&width.to_be_bytes());
    header[4..8].copy_from_slice(&height.to_be_bytes());
    header[8] = bits;
    header[9] = color as u8;
    chunk(&mut out, *b"IHDR", &header);
    if let Some(palette) = palette {
        chunk(&mut out, *b"PLTE", palette);
    }
    chunk(&mut out, *b"IDAT", &compressed);
    chunk(&mut out, *b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = crc32fast::hash(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// How a fax-compressed image is coded (`CCITTFaxDecode`'s `K`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fax {
    Group3OneDimensional,
    Group3TwoDimensional,
    Group4,
}

/// A one-strip TIFF around CCITT fax data, as PDFs store scans.
/// `black_is_one` is whether 1 bits are black (`BlackIs1`, after `Decode`).
pub fn tiff(
    width: u32,
    height: u32,
    fax: Fax,
    black_is_one: bool,
    byte_aligned: bool,
    data: &[u8],
) -> Vec<u8> {
    let (compression, options_tag, options) = match fax {
        Fax::Group3OneDimensional => (3, 292, u32::from(byte_aligned) << 2),
        Fax::Group3TwoDimensional => (3, 292, 1 | u32::from(byte_aligned) << 2),
        Fax::Group4 => (4, 293, 0),
    };
    // TIFF photometric: 0 is WhiteIsZero, 1 is BlackIsZero.
    let photometric = u32::from(!black_is_one);
    let entries: [(u16, u16, u32); 10] = [
        (256, 4, width),
        (257, 4, height),
        (258, 3, 1),
        (259, 3, compression),
        (262, 3, photometric),
        (273, 4, 0), // strip offset, set below
        (277, 3, 1),
        (278, 4, height),
        (279, 4, u32::try_from(data.len()).unwrap_or(u32::MAX)),
        (options_tag, 4, options),
    ];
    let directory = 8;
    let strip = directory + 2 + entries.len() * 12 + 4;
    let mut out = Vec::with_capacity(strip + data.len());
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&u32::try_from(directory).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&u16::try_from(entries.len()).unwrap_or(0).to_le_bytes());
    for (tag, kind, value) in entries {
        let value = if tag == 273 { u32::try_from(strip).unwrap_or(0) } else { value };
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        // A SHORT sits in the first two bytes of the value field.
        if kind == 3 {
            out.extend_from_slice(&u16::try_from(value).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(data);
    out
}
