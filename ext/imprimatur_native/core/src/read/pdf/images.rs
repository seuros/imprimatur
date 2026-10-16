//! A PDF's images as files (ISO 32000-1, 8.9). JPEG and JPEG 2000 streams
//! are kept as they are, fax-compressed scans are wrapped in a TIFF, and
//! raw pixels become PNG: gray, RGB, CMYK (converted), indexed, with their
//! soft mask as alpha when it matches. Each image is extracted once, from
//! the first page that draws it.

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use super::content::{Drawing, page_images};
use super::document::{Document, decode_with};
use super::object::{Dict, Object};
use super::xref::is_limit;
use crate::Result;
use crate::read::Budget;
use crate::read::image::{Fax, Image, ImageFormat, ImageLocation, PngColor, png, tiff};

/// Calls `each` with every image the document's pages draw.
pub fn images(
    data: &[u8],
    budget: &Budget<'_>,
    each: &mut dyn FnMut(Image) -> Result<()>,
) -> Result<()> {
    let document = Document::open(data, budget)?;
    let pages = document.pages()?;
    let mut seen = HashSet::new();
    let mut drawn = Vec::new();
    for (number, page) in (1..).zip(&pages) {
        budget.check()?;
        drawn.clear();
        let frame = match page_images(&document, page, &mut drawn) {
            Ok(frame) => frame,
            Err(error) if is_limit(&error) => return Err(error),
            // A damaged page loses its images, not the document's.
            Err(_) => continue,
        };
        for item in &drawn {
            let encoded = match &item.image {
                Drawing::Object { number: Some(object), .. } if !seen.insert(*object) => continue,
                Drawing::Object { number, object } => {
                    let Object::Stream(stream) = &**object else { continue };
                    document
                        .raw(stream, *number)
                        .and_then(|raw| encode(&document, &stream.dict, &raw))
                }
                Drawing::Inline { dict, data } => encode(&document, dict, data),
            };
            let encoded = match encoded {
                Ok(Some(encoded)) => encoded,
                Err(error) if is_limit(&error) => return Err(error),
                Ok(None) | Err(_) => continue,
            };
            let bounds = Some(frame.place(&item.ctm));
            each(Image {
                data: encoded.data,
                format: encoded.format,
                width: Some(encoded.width),
                height: Some(encoded.height),
                location: ImageLocation::Page { number, bounds },
            })?;
        }
    }
    Ok(())
}

struct Encoded {
    data: Vec<u8>,
    format: ImageFormat,
    width: u32,
    height: u32,
}

/// An image stream's data (decrypted) as a file; `None` for what cannot
/// be written as one (Lab colour, unknown spaces, broken dimensions).
fn encode(document: &Document<'_>, dict: &Dict, raw: &[u8]) -> Result<Option<Encoded>> {
    let number = |key: &[u8]| -> Result<Option<f64>> {
        Ok(document.entry(dict, key)?.and_then(|value| value.as_number()))
    };
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "1 to 1e6")]
    let dimension = |key: &[u8]| -> Result<Option<u32>> {
        Ok(number(key)?.filter(|value| (1.0..=1e6).contains(value)).map(|value| value as u32))
    };
    let (Some(width), Some(height)) = (dimension(b"Width")?, dimension(b"Height")?) else {
        return Ok(None);
    };
    let mut filters = document.filters(dict)?;
    let codec = filters.last().and_then(|(name, _)| match &**name {
        b"DCTDecode" | b"DCT" => Some(ImageFormat::Jpeg),
        b"JPXDecode" => Some(ImageFormat::Jpeg2000),
        b"JBIG2Decode" => Some(ImageFormat::Jbig2),
        b"CCITTFaxDecode" | b"CCF" => Some(ImageFormat::Tiff),
        _ => None,
    });
    let params = if codec.is_some() { filters.pop().and_then(|(_, params)| params) } else { None };
    let data = decode_with(raw, &filters, document.budget())?;
    let params = params.as_deref().and_then(Object::as_dict);
    let encoded = |data, format| Ok(Some(Encoded { data, format, width, height }));
    match codec {
        Some(format @ (ImageFormat::Jpeg | ImageFormat::Jpeg2000)) => encoded(data, format),
        Some(ImageFormat::Jbig2) => {
            let globals = match params.and_then(|params| params.get(b"JBIG2Globals")) {
                Some(object) => document.stream_data(object)?.unwrap_or_default(),
                None => Vec::new(),
            };
            encoded([globals, data].concat(), ImageFormat::Jbig2)
        }
        Some(_) => {
            let value = |key: &[u8]| params.and_then(|params| params.get(key));
            let k = value(b"K").and_then(Object::as_integer).unwrap_or(0);
            let fax = match k {
                ..0 => Fax::Group4,
                0 => Fax::Group3OneDimensional,
                _ => Fax::Group3TwoDimensional,
            };
            let black_is_one = value(b"BlackIs1") == Some(&Object::Bool(true));
            let aligned = value(b"EncodedByteAlign") == Some(&Object::Bool(true));
            let flipped = inverted(document, dict)?;
            let data = tiff(width, height, fax, black_is_one != flipped, aligned, &data);
            encoded(data, ImageFormat::Tiff)
        }
        None => pixels(document, dict, &data, width, height),
    }
}

/// Whether `Decode` is `[1 0]`: a one-component image drawn inverted.
fn inverted(document: &Document<'_>, dict: &Dict) -> Result<bool> {
    let decode = document.entry(dict, b"Decode")?;
    let decode = decode.as_deref().and_then(Object::as_array).unwrap_or_default();
    let bound = |object: &Object| object.as_number();
    Ok(matches!(decode, [low, high] if bound(low) == Some(1.0) && bound(high) == Some(0.0)))
}

/// What colour space a raw image's samples are in, as far as writing it
/// out goes.
enum Space {
    Gray,
    /// One ink: 1 is full ink, so dark.
    Ink,
    Rgb,
    Cmyk,
    /// Indices into a palette of RGB triples.
    Indexed(Vec<u8>),
}

impl Space {
    const fn components(&self) -> usize {
        match self {
            Self::Gray | Self::Ink | Self::Indexed(_) => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

fn space(document: &Document<'_>, object: &Object, depth: usize) -> Result<Option<Space>> {
    let object = document.resolve(object)?;
    let name = match &*object {
        Object::Name(name) => Some(&**name),
        Object::Array(items) => items.first().and_then(Object::as_name),
        _ => None,
    };
    let items = object.as_array().unwrap_or_default();
    Ok(match name {
        Some(b"DeviceGray" | b"CalGray" | b"G") => Some(Space::Gray),
        Some(b"DeviceRGB" | b"CalRGB" | b"RGB") => Some(Space::Rgb),
        Some(b"DeviceCMYK" | b"CMYK") => Some(Space::Cmyk),
        Some(b"Separation") => Some(Space::Ink),
        Some(b"DeviceN") => {
            let inks = items.get(1).map(|names| document.resolve(names)).transpose()?;
            (inks.as_deref().and_then(Object::as_array).map(<[Object]>::len) == Some(1))
                .then_some(Space::Ink)
        }
        Some(b"ICCBased") => {
            let profile = items.get(1).map(|profile| document.resolve(profile)).transpose()?;
            let dict = profile.as_deref().and_then(Object::as_dict);
            match dict.and_then(|dict| dict.get(b"N")).and_then(Object::as_integer) {
                Some(1) => Some(Space::Gray),
                Some(3) => Some(Space::Rgb),
                Some(4) => Some(Space::Cmyk),
                _ => match dict.and_then(|dict| dict.get(b"Alternate")) {
                    Some(alternate) if depth < 4 => space(document, alternate, depth + 1)?,
                    _ => None,
                },
            }
        }
        Some(b"Indexed" | b"I") if depth < 4 && items.len() >= 4 => {
            let Some(base) = space(document, &items[1], depth + 1)? else { return Ok(None) };
            let entries =
                usize::try_from(items[2].as_integer().unwrap_or(0)).unwrap_or(0).min(255) + 1;
            let lookup = match document.resolve(&items[3])?.as_bytes() {
                Some(bytes) => bytes.to_vec(),
                None => document.stream_data(&items[3])?.unwrap_or_default(),
            };
            let mut palette = Vec::with_capacity(entries * 3);
            for entry in lookup.chunks(base.components()).take(entries) {
                push_rgb(&mut palette, &base, entry);
            }
            palette.resize(entries * 3, 0);
            Some(Space::Indexed(palette))
        }
        _ => None,
    })
}

/// One sample set as RGB.
fn push_rgb(out: &mut Vec<u8>, space: &Space, samples: &[u8]) {
    let sample = |index: usize| samples.get(index).copied().unwrap_or(0);
    match space {
        Space::Gray => out.extend_from_slice(&[sample(0); 3]),
        Space::Ink => out.extend_from_slice(&[!sample(0); 3]),
        Space::Rgb => out.extend_from_slice(&[sample(0), sample(1), sample(2)]),
        Space::Cmyk => {
            let ink = |value: u8| {
                u8::try_from(u16::from(255 - value) * u16::from(255 - sample(3)) / 255).unwrap_or(0)
            };
            out.extend_from_slice(&[ink(sample(0)), ink(sample(1)), ink(sample(2))]);
        }
        Space::Indexed(palette) => {
            let at = usize::from(sample(0)) * 3;
            out.extend_from_slice(palette.get(at..at + 3).unwrap_or(&[0, 0, 0]));
        }
    }
}

/// Raw samples as a PNG.
fn pixels(
    document: &Document<'_>,
    dict: &Dict,
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<Option<Encoded>> {
    let mask =
        document.entry(dict, b"ImageMask")?.is_some_and(|value| *value == Object::Bool(true));
    let bits = if mask {
        1
    } else {
        let bits = document.entry(dict, b"BitsPerComponent")?.and_then(|value| value.as_integer());
        match bits.unwrap_or(8) {
            bits @ (1 | 2 | 4 | 8 | 16) => u8::try_from(bits).unwrap_or(8),
            _ => return Ok(None),
        }
    };
    let space = if mask {
        Space::Gray
    } else {
        match dict.get(b"ColorSpace") {
            Some(object) => match space(document, object, 0)? {
                Some(space) => space,
                None => return Ok(None),
            },
            None => return Ok(None),
        }
    };
    let (pixels, rows) = (width as usize * height as usize, height as usize);
    let stride = (width as usize * space.components() * usize::from(bits)).div_ceil(8);
    // What is decoded beyond the stream: rows as stored, or 4 bytes a pixel.
    document.budget().inflate(stride.saturating_mul(rows).max(pixels.saturating_mul(4)))?;
    let alpha = soft_mask(document, dict, width, height)?;
    // An ink's 1 is dark; `Decode [1 0]` turns it around again.
    let flip = inverted(document, dict)? != matches!(space, Space::Ink);
    let one = |png| Ok(Some(Encoded { data: png, format: ImageFormat::Png, width, height }));
    // As stored, when PNG can say it the same way.
    let stored = data.get(..stride * rows).unwrap_or(data);
    match (&space, &alpha) {
        (Space::Gray | Space::Ink, None) => {
            let flipped: Vec<u8>;
            let samples = if flip {
                flipped = stored.iter().map(|byte| !byte).collect();
                &flipped
            } else {
                stored
            };
            return one(png(width, height, PngColor::Gray, bits, None, samples));
        }
        (Space::Rgb, None) if bits >= 8 => {
            return one(png(width, height, PngColor::Rgb, bits, None, stored));
        }
        (Space::Indexed(palette), None) if bits <= 8 => {
            return one(png(width, height, PngColor::Palette, bits, Some(palette), stored));
        }
        _ => {}
    }
    // Otherwise, sample by sample: 8 bits, RGB or gray, alpha if any.
    let samples =
        unpack(data, width, height, space.components(), bits, !matches!(space, Space::Indexed(_)));
    let gray = matches!(space, Space::Gray | Space::Ink);
    let mut out = Vec::with_capacity(pixels * if gray { 2 } else { 4 });
    for (index, set) in samples.chunks(space.components()).take(pixels).enumerate() {
        if gray {
            out.push(if flip { !set[0] } else { set[0] });
        } else {
            push_rgb(&mut out, &space, set);
        }
        if let Some(alpha) = &alpha {
            out.push(alpha.get(index).copied().unwrap_or(255));
        }
    }
    let color = match (gray, alpha.is_some()) {
        (true, false) => PngColor::Gray,
        (true, true) => PngColor::GrayAlpha,
        (false, false) => PngColor::Rgb,
        (false, true) => PngColor::Rgba,
    };
    one(png(width, height, color, 8, None, &out))
}

/// A soft mask that matches the image, 8 bits a pixel, as alpha.
fn soft_mask(
    document: &Document<'_>,
    dict: &Dict,
    width: u32,
    height: u32,
) -> Result<Option<Vec<u8>>> {
    let Some(reference) = dict.get(b"SMask") else { return Ok(None) };
    let resolved = document.resolve(reference)?;
    let Object::Stream(stream) = &*resolved else { return Ok(None) };
    let size = |key: &[u8]| stream.dict.get(key).and_then(Object::as_integer);
    let matches = size(b"Width") == Some(i64::from(width))
        && size(b"Height") == Some(i64::from(height))
        && size(b"BitsPerComponent").unwrap_or(8) == 8;
    if !matches {
        return Ok(None);
    }
    let number = reference.as_reference().map(|(number, _)| number);
    let Ok(alpha) = document.decode(stream, number) else { return Ok(None) };
    Ok((alpha.len() >= width as usize * height as usize).then_some(alpha))
}

/// Packed samples as one byte each; `scale` stretches fewer than 8 bits to
/// 0..=255 (indices keep their value), 16 bits keep their high byte.
fn unpack(
    data: &[u8],
    width: u32,
    height: u32,
    components: usize,
    bits: u8,
    scale: bool,
) -> Vec<u8> {
    let per_row = width as usize * components;
    let stride = (per_row * usize::from(bits)).div_ceil(8);
    let mut out = Vec::with_capacity(per_row * height as usize);
    let max = (1_u16 << bits.min(8)) - 1;
    for row in 0..height as usize {
        let line = data.get(row * stride..(row + 1) * stride).unwrap_or_default();
        for index in 0..per_row {
            let value = match bits {
                16 => line.get(index * 2).copied().unwrap_or(0),
                8 => line.get(index).copied().unwrap_or(0),
                _ => {
                    let bit = index * usize::from(bits);
                    let byte = line.get(bit / 8).copied().unwrap_or(0);
                    let shift = 8 - usize::from(bits) - bit % 8;
                    let value = u16::from(byte >> shift) & max;
                    let value = if scale { value * 255 / max } else { value };
                    u8::try_from(value).unwrap_or(u8::MAX)
                }
            };
            out.push(value);
        }
    }
    out
}
