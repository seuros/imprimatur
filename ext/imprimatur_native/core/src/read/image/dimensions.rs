//! Image sizes from file headers, for the formats that put them up front.
//! Vector formats give the size they are drawn at, in pixels at 96 dpi.

use super::ImageFormat;
use crate::xml::reader::Tag;

/// Pixels per inch, for vector formats sized in physical units.
const DPI: f64 = 96.0;

/// Width and height, when the header says them.
pub fn dimensions(format: ImageFormat, data: &[u8]) -> Option<(u32, u32)> {
    let bytes = Bytes(data);
    match format {
        ImageFormat::Png => Some((bytes.u32_be(16)?, bytes.u32_be(20)?)),
        ImageFormat::Gif => Some((bytes.u16_le(6)?.into(), bytes.u16_le(8)?.into())),
        ImageFormat::Bmp => {
            Some((bytes.i32_le(18)?.unsigned_abs(), bytes.i32_le(22)?.unsigned_abs()))
        }
        ImageFormat::Jpeg => jpeg(&bytes),
        ImageFormat::Tiff => tiff(&bytes),
        ImageFormat::Webp => webp(&bytes),
        ImageFormat::Jpeg2000 => jpeg2000(&bytes),
        ImageFormat::Emf => emf(&bytes),
        ImageFormat::Wmf => wmf(&bytes),
        ImageFormat::Svg => svg(data),
        _ => None,
    }
    .filter(|&(width, height)| width > 0 && height > 0)
}

/// Bytes read at offsets; `None` past the end.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn array<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.0.get(at..at.checked_add(N)?)?.try_into().ok()
    }

    fn u16_le(&self, at: usize) -> Option<u16> {
        self.array(at).map(u16::from_le_bytes)
    }

    fn u16_be(&self, at: usize) -> Option<u16> {
        self.array(at).map(u16::from_be_bytes)
    }

    fn u32_le(&self, at: usize) -> Option<u32> {
        self.array(at).map(u32::from_le_bytes)
    }

    fn u32_be(&self, at: usize) -> Option<u32> {
        self.array(at).map(u32::from_be_bytes)
    }

    fn i32_le(&self, at: usize) -> Option<i32> {
        self.array(at).map(i32::from_le_bytes)
    }

    fn i16_le(&self, at: usize) -> Option<i16> {
        self.array(at).map(i16::from_le_bytes)
    }

    /// A 24-bit little-endian number.
    fn u24_le(&self, at: usize) -> Option<u32> {
        let [low, middle, high] = self.array(at)?;
        Some(u32::from_le_bytes([low, middle, high, 0]))
    }
}

/// From the start-of-frame marker.
fn jpeg(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        let [mark, marker] = bytes.array(at)?;
        if mark != 0xFF {
            return None;
        }
        // SOF0 to SOF15, but DHT, JPG and DAC share the range.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((bytes.u16_be(at + 7)?.into(), bytes.u16_be(at + 5)?.into()));
        }
        at += 2 + usize::from(bytes.u16_be(at + 2)?);
    }
}

/// From the first image directory's `ImageWidth` and `ImageLength`.
fn tiff(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    let little = bytes.0.starts_with(b"II");
    let short = |at| if little { bytes.u16_le(at) } else { bytes.u16_be(at) };
    let long = |at| if little { bytes.u32_le(at) } else { bytes.u32_be(at) };
    let directory = usize::try_from(long(4)?).ok()?;
    let (mut width, mut height) = (None, None);
    for index in 0..usize::from(short(directory)?) {
        let entry = directory + 2 + index * 12;
        let value = match short(entry + 2)? {
            3 => u32::from(short(entry + 8)?),
            4 => long(entry + 8)?,
            _ => continue,
        };
        match short(entry)? {
            256 => width = Some(value),
            257 => height = Some(value),
            _ => {}
        }
        if let (Some(width), Some(height)) = (width, height) {
            return Some((width, height));
        }
    }
    None
}

/// From the first chunk: lossy, lossless or extended.
fn webp(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    match &bytes.array(12)? {
        b"VP8 " => {
            let size = |at| bytes.u16_le(at).map(|value| u32::from(value & 0x3FFF));
            Some((size(26)?, size(28)?))
        }
        b"VP8L" => {
            let bits = bytes.u32_le(21)?;
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((bytes.u24_le(24)? + 1, bytes.u24_le(27)? + 1)),
        _ => None,
    }
}

/// From a codestream's SIZ marker, or a JP2 file's image header box.
fn jpeg2000(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    if bytes.0.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]) {
        let width = bytes.u32_be(8)?.checked_sub(bytes.u32_be(16)?)?;
        let height = bytes.u32_be(12)?.checked_sub(bytes.u32_be(20)?)?;
        return Some((width, height));
    }
    let header = Bytes(find_box(find_box(bytes.0, *b"jp2h")?, *b"ihdr")?);
    Some((header.u32_be(4)?, header.u32_be(0)?))
}

/// The contents of the first box of `kind` among `data`'s boxes.
fn find_box(mut data: &[u8], kind: [u8; 4]) -> Option<&[u8]> {
    while data.len() >= 8 {
        let bytes = Bytes(data);
        let (start, length) = match bytes.u32_be(0)? {
            0 => (8, data.len()),
            1 => (16, usize::try_from(u64::from_be_bytes(bytes.array(8)?)).ok()?),
            length => (8, usize::try_from(length).ok()?),
        };
        if length < start || length > data.len() {
            return None;
        }
        if bytes.array(4)? == kind {
            return data.get(start..length);
        }
        data = &data[length..];
    }
    None
}

/// From the header record's bounds, in device units, inclusive.
fn emf(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    let span = |low: i32, high: i32| u32::try_from(i64::from(high) - i64::from(low) + 1).ok();
    Some((span(bytes.i32_le(8)?, bytes.i32_le(16)?)?, span(bytes.i32_le(12)?, bytes.i32_le(20)?)?))
}

/// From a placeable metafile's bounding box and its units per inch.
fn wmf(bytes: &Bytes<'_>) -> Option<(u32, u32)> {
    let inch = f64::from(bytes.u16_le(14)?);
    let span =
        |low: i16, high: i16| pixels(f64::from(i32::from(high) - i32::from(low)) / inch * DPI);
    if inch == 0.0 {
        return None;
    }
    Some((span(bytes.i16_le(6)?, bytes.i16_le(10)?)?, span(bytes.i16_le(8)?, bytes.i16_le(12)?)?))
}

/// From the root element's `width` and `height`, or its `viewBox`.
fn svg(data: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(data.get(..data.len().min(64 * 1024))?).ok()?;
    let start = text.match_indices("<svg").map(|(at, _)| at + 4).find(|&at| {
        text[at..].starts_with(|ch: char| ch.is_ascii_whitespace() || ch == '>' || ch == '/')
    })?;
    let tag = Tag::new("svg", &text[start..start + text[start..].find('>')?]);
    let length = |name| tag.attr(name).and_then(|value| svg_length(&value));
    if let (Some(width), Some(height)) = (length("width"), length("height")) {
        return Some((width, height));
    }
    let view = tag.attr("viewBox")?;
    let mut numbers = view.split([' ', ',']).filter(|part| !part.is_empty()).skip(2);
    let mut next = || numbers.next()?.parse::<f64>().ok().and_then(pixels);
    Some((next()?, next()?))
}

/// An SVG length in pixels; `None` for relative units (`%`, `em`).
fn svg_length(value: &str) -> Option<u32> {
    let value = value.trim();
    let split = value.find(|ch: char| ch.is_ascii_alphabetic() || ch == '%').unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let scale = match unit {
        "" | "px" => 1.0,
        "pt" => DPI / 72.0,
        "pc" => DPI / 6.0,
        "in" => DPI,
        "cm" => DPI / 2.54,
        "mm" => DPI / 25.4,
        _ => return None,
    };
    pixels(number * scale)
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to fit")]
fn pixels(value: f64) -> Option<u32> {
    let value = value.round();
    (value.is_finite() && (1.0..=f64::from(u32::MAX)).contains(&value)).then_some(value as u32)
}
