//! Fonts as text extraction needs them (ISO 32000-1, 9.5 to 9.10): how a
//! string splits into character codes, each code's text and width, and
//! whether the face is bold.

#[cfg(test)]
mod tests;

use std::ops::Range;

use super::cmap::{CMap, utf16};
use super::document::{Document, Resolved};
use super::glyphs::{BaseEncoding, encoded, named};
use super::object::{Dict, Object};
use crate::Result;
use crate::pdf::metrics::{HELVETICA, HELVETICA_BOLD};
use crate::read::span;

/// Widths in glyph space units: thousandths of the font size.
const DEFAULT_WIDTH: f64 = 500.0;

#[derive(Debug)]
enum Codes {
    /// One byte per code; each code's text from the encoding.
    Simple { text: Box<[Range<u32>; 256]>, widths: Box<[f64; 256]> },
    /// Codes per the encoding `CMap` (two bytes for `Identity-H`).
    Composite {
        encoding: Option<CMap>,
        identity: bool,
        unicode: bool,
        default: f64,
        widths: Vec<(u32, u32, f64)>,
    },
}

#[derive(Debug)]
pub struct Font {
    pub bold: bool,
    codes: Codes,
    to_unicode: Option<CMap>,
    /// The simple encoding's text, back to back.
    text: String,
    /// Glyph space to text space: 0.001, or a Type 3 font's matrix.
    scale: f64,
}

impl Font {
    /// A font from its dictionary. Never fails on a damaged font: what
    /// cannot be read falls back to defaults.
    pub fn load(document: &Document<'_>, dict: &Dict) -> Result<Self> {
        let cmap = |object: &Object| -> Result<Option<CMap>> {
            Ok(document.stream_data(object)?.map(|data| CMap::parse(&data)))
        };
        let to_unicode = dict.get(b"ToUnicode").map(cmap).transpose()?.flatten();
        let base_font = document.entry(dict, b"BaseFont")?;
        let base_font = base_font.as_deref().and_then(Object::as_name).unwrap_or_default();
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).unwrap_or_default();
        // A composite font's widths and descriptor are its descendant's.
        let descendant = if subtype == b"Type0" {
            match document.entry(dict, b"DescendantFonts")?.as_deref() {
                Some(Object::Array(fonts)) => fonts
                    .first()
                    .map(|font| document.resolve(font).map(Resolved::into_shared))
                    .transpose()?,
                _ => None,
            }
        } else {
            None
        };
        let empty = Dict::default();
        let described = descendant.as_deref().and_then(Object::as_dict).unwrap_or(dict);
        let descriptor = document.entry(described, b"FontDescriptor")?;
        let descriptor = descriptor.as_deref().and_then(Object::as_dict).unwrap_or(&empty);
        let flags = descriptor.get(b"Flags").and_then(Object::as_integer).unwrap_or(0);
        let mut text = String::new();
        let (codes, scale) = if subtype == b"Type0" {
            let (encoding, identity, unicode) = match dict.get(b"Encoding") {
                Some(Object::Name(name)) => (
                    None,
                    name.starts_with(b"Identity"),
                    name.windows(4).any(|part| part == b"UCS2" || part == b"TF16"),
                ),
                Some(object) => (cmap(object)?, false, false),
                None => (None, true, false),
            };
            let default = document
                .entry(described, b"DW")?
                .and_then(|width| width.as_number())
                .unwrap_or(1000.0);
            let widths = match document.entry(described, b"W")?.as_deref() {
                Some(Object::Array(items)) => cid_widths(document, items)?,
                _ => Vec::new(),
            };
            (Codes::Composite { encoding, identity, unicode, default, widths }, 0.001)
        } else {
            let symbolic = flags & 4 != 0;
            let table = simple_text(document, dict, base_font, subtype, symbolic, &mut text)?;
            let scale = if subtype == b"Type3" {
                let matrix = document.entry(dict, b"FontMatrix")?;
                let first =
                    matrix.as_deref().and_then(Object::as_array).and_then(|matrix| matrix.first());
                first
                    .and_then(Object::as_number)
                    .filter(|scale| *scale != 0.0)
                    .unwrap_or(0.001)
                    .abs()
            } else {
                0.001
            };
            let widths = simple_widths(document, dict, base_font, descriptor)?;
            (Codes::Simple { text: table, widths }, scale)
        };
        let weight = descriptor.get(b"FontWeight").and_then(Object::as_number).unwrap_or(0.0);
        let bold = weight >= 600.0
            || flags & 0x4_0000 != 0
            || [&b"bold"[..], b"black", b"heavy", b"semibold", b"demi"].iter().any(|word| {
                base_font.windows(word.len()).any(|part| part.eq_ignore_ascii_case(word))
            });
        Ok(Self { bold, codes, to_unicode, text, scale })
    }

    /// Calls `glyph` for each character code in `bytes`: its text (empty
    /// when unknown), its width in text space units per unit of font size,
    /// and whether it is the single-byte code 32 that word spacing applies to.
    pub fn glyphs(
        &self,
        bytes: &[u8],
        scratch: &mut String,
        mut glyph: impl FnMut(&str, f64, bool),
    ) {
        let mut at = 0;
        while at < bytes.len() {
            let length = match &self.codes {
                Codes::Simple { .. } => 1,
                Codes::Composite { encoding: Some(cmap), .. } if cmap.has_spaces() => {
                    cmap.code_length(&bytes[at..])
                }
                Codes::Composite { .. } => self
                    .to_unicode
                    .as_ref()
                    .filter(|cmap| cmap.has_spaces())
                    .map_or(2, |cmap| cmap.code_length(&bytes[at..])),
            };
            let code_bytes = &bytes[at..(at + length).min(bytes.len())];
            at += length;
            let code = super::cmap::be(code_bytes);
            scratch.clear();
            let mapped = self.to_unicode.as_ref().is_some_and(|cmap| cmap.push_text(code, scratch));
            let width = match &self.codes {
                Codes::Simple { text, widths } => {
                    if !mapped {
                        let span = &text[code as usize & 0xFF];
                        scratch.push_str(span::slice(&self.text, span));
                    }
                    widths[code as usize & 0xFF]
                }
                Codes::Composite { encoding, identity, unicode, default, widths } => {
                    if !mapped && *unicode {
                        scratch.push_str(&utf16(code_bytes));
                    }
                    let cid = if *identity {
                        Some(code)
                    } else {
                        encoding.as_ref().and_then(|cmap| cmap.cid(code))
                    };
                    cid.and_then(|cid| {
                        widths
                            .iter()
                            .find(|&&(low, high, _)| (low..=high).contains(&cid))
                            .map(|&(_, _, width)| width)
                    })
                    .unwrap_or(*default)
                }
            };
            glyph(scratch, width * self.scale, length == 1 && code == 32);
        }
    }
}

/// The text of each code of a simple font: its base encoding (named, or
/// the font's built-in one), then the `/Differences` over it.
fn simple_text(
    document: &Document<'_>,
    dict: &Dict,
    base_font: &[u8],
    subtype: &[u8],
    symbolic: bool,
    text: &mut String,
) -> Result<Box<[Range<u32>; 256]>> {
    let built_in = if base_font.windows(6).any(|part| part == b"Symbol") {
        BaseEncoding::Symbol
    } else if base_font.windows(7).any(|part| part == b"Dingbat") {
        BaseEncoding::ZapfDingbats
    } else if subtype == b"Type1" && !symbolic {
        BaseEncoding::Standard
    } else {
        BaseEncoding::WinAnsi
    };
    let encoding = document.entry(dict, b"Encoding")?;
    let (base, differences) = match encoding.as_deref() {
        Some(Object::Name(name)) => (named_encoding(name).unwrap_or(built_in), None),
        Some(Object::Dict(encoding)) => {
            let base = encoding
                .get(b"BaseEncoding")
                .and_then(Object::as_name)
                .and_then(named_encoding)
                .unwrap_or(built_in);
            (
                base,
                document
                    .entry(encoding, b"Differences")?
                    .and_then(|differences| differences.as_array().map(<[Object]>::to_vec)),
            )
        }
        _ => (built_in, None),
    };
    let mut table: Box<[Range<u32>; 256]> = Box::new(std::array::from_fn(|_| 0..0));
    let store = |text: &mut String, glyph: &str| span::append(text, |text| text.push_str(glyph));
    for code in 0..=255_u8 {
        table[usize::from(code)] = store(text, encoded(base, code));
    }
    let mut code = 0_usize;
    for item in differences.unwrap_or_default() {
        match item {
            Object::Integer(start) => code = usize::try_from(start).unwrap_or(256),
            Object::Name(name) => {
                if code < 256 {
                    let glyph = std::str::from_utf8(&name).ok().and_then(named);
                    table[code] = store(text, glyph.as_deref().unwrap_or(""));
                }
                code += 1;
            }
            _ => {}
        }
    }
    Ok(table)
}

const fn named_encoding(name: &[u8]) -> Option<BaseEncoding> {
    match name {
        b"WinAnsiEncoding" => Some(BaseEncoding::WinAnsi),
        b"MacRomanEncoding" => Some(BaseEncoding::MacRoman),
        b"StandardEncoding" | b"MacExpertEncoding" => Some(BaseEncoding::Standard),
        _ => None,
    }
}

/// `/FirstChar` and `/Widths`; the missing width, or the standard font's
/// widths, for codes they leave out.
fn simple_widths(
    document: &Document<'_>,
    dict: &Dict,
    base_font: &[u8],
    descriptor: &Dict,
) -> Result<Box<[f64; 256]>> {
    let missing =
        descriptor.get(b"MissingWidth").and_then(Object::as_number).filter(|width| *width > 0.0);
    let standard =
        if base_font.windows(4).any(|part| part == b"Bold") { &HELVETICA_BOLD } else { &HELVETICA };
    let courier = base_font.starts_with(b"Courier");
    let mut widths: Box<[f64; 256]> = Box::new(std::array::from_fn(|code| {
        missing.unwrap_or_else(|| {
            if courier {
                600.0
            } else {
                f64::from(standard.widths[code]).max(if code == 32 {
                    250.0
                } else {
                    DEFAULT_WIDTH * 0.5
                })
            }
        })
    }));
    let first =
        document.entry(dict, b"FirstChar")?.and_then(|first| first.as_integer()).unwrap_or(0);
    if let Some(listed) = document.entry(dict, b"Widths")?
        && let Some(listed) = listed.as_array()
    {
        for (offset, width) in listed.iter().enumerate() {
            let code = usize::try_from(first).unwrap_or(0) + offset;
            if code < 256
                && let Some(width) = document.resolve(width)?.as_number()
            {
                widths[code] = width;
            }
        }
    }
    Ok(widths)
}

/// A CID font's `/W`: `first [w1 w2 ...]` and `first last w` entries.
fn cid_widths(document: &Document<'_>, items: &[Object]) -> Result<Vec<(u32, u32, f64)>> {
    let mut widths = Vec::new();
    let mut at = 0;
    while at + 1 < items.len() {
        let first = items[at].as_integer().and_then(|first| u32::try_from(first).ok()).unwrap_or(0);
        if let Some(list) = document.resolve(&items[at + 1])?.as_array() {
            for (offset, width) in (0..).zip(list) {
                if let Some(width) = width.as_number() {
                    widths.push((first + offset, first + offset, width));
                }
            }
            at += 2;
        } else {
            let last = items[at + 1]
                .as_integer()
                .and_then(|last| u32::try_from(last).ok())
                .unwrap_or(first);
            if let Some(width) = items.get(at + 2).and_then(Object::as_number) {
                widths.push((first, last, width));
            }
            at += 3;
        }
    }
    Ok(widths)
}
