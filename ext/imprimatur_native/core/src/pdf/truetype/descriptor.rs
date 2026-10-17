//! TrueType metadata needed by a PDF `FontDescriptor`, in 1000-unit glyph space.

#[cfg(test)]
mod tests;

use std::io::Write;

use read_fonts::{
    FontRead, FontRef, TableProvider,
    tables::{head::MacStyle, os2::Os2, post::Post},
    types::{NameId, Tag},
};

use crate::font::TrueTypeFont;
use crate::pdf::objects::{Num, PdfWriter};
use crate::{Error, Result};

pub struct Descriptor {
    /// PostScript name, escaped as a PDF name (without its leading slash).
    pub name: String,
    bounds: [Num; 4],
    ascent: Num,
    descent: Num,
    cap_height: Num,
    italic_angle: Num,
    flags: u32,
    stem: Num,
}

impl Descriptor {
    pub fn read(font: &TrueTypeFont<'_>) -> Result<Self> {
        let tables = FontRef::new(font.as_bytes()).map_err(invalid)?;
        let head = tables.head().map_err(invalid)?;
        let post =
            tables.table_data(Tag::new(b"post")).map(Post::read).transpose().map_err(invalid)?;
        let os2 =
            tables.table_data(Tag::new(b"OS/2")).map(Os2::read).transpose().map_err(invalid)?;
        if let Some(os2) = &os2 {
            let rights = os2.fs_type();
            // Older fonts can set several rights bits; the least restrictive wins.
            (rights & 0x0002 == 0 || rights & 0x000C != 0)
                .ok_or(Error::UnsupportedFont("restricted font embedding"))?;
            (rights & 0x0200 == 0).ok_or(Error::UnsupportedFont("bitmap-only font embedding"))?;
            // The no-subsetting bit is compatible with embedding the complete font.
        }
        (head.x_min() <= head.x_max() && head.y_min() <= head.y_max())
            .ok_or_else(|| invalid("inverted font bounding box"))?;
        let metrics = font.metrics();
        let scale = 1000.0 / f64::from(metrics.units_per_em);
        let cap_height = os2
            .as_ref()
            .and_then(Os2::s_cap_height)
            .filter(|height| *height > 0)
            .unwrap_or(metrics.ascender);
        let angle = post.as_ref().map_or(0.0, |post| post.italic_angle().to_f64());
        // Symbolic: Identity-H is a CID encoding, not the standard Latin encoding.
        let mut flags = 4;
        if post.as_ref().is_some_and(|post| post.is_fixed_pitch() != 0) {
            flags |= 1;
        }
        if angle != 0.0 || head.mac_style().contains(MacStyle::ITALIC) {
            flags |= 64;
        }
        let family = os2.as_ref().map_or(0, |os2| os2.s_family_class() >> 8);
        if (1..=7).contains(&family) {
            flags |= 2;
        } else if family == 10 {
            flags |= 8;
        }
        let weight = os2.as_ref().map_or(400, |os2| os2.us_weight_class().clamp(1, 1000));
        let stem_weight = f64::from(weight) / 65.0;
        Ok(Self {
            name: postscript_name(&tables)?,
            bounds: [head.x_min(), head.y_min(), head.x_max(), head.y_max()]
                .map(|value| Num(f64::from(value) * scale)),
            ascent: Num(f64::from(metrics.ascender) * scale),
            descent: Num(f64::from(metrics.descender) * scale),
            // Older OS/2 tables lack cap height; the ascender is an approximation.
            cap_height: Num(f64::from(cap_height) * scale),
            italic_angle: Num(angle),
            flags,
            // TrueType has no stem-width field: estimate it from the weight class.
            stem: Num(stem_weight.mul_add(stem_weight, 50.0)),
        })
    }

    pub fn write<W: Write>(&self, pdf: &mut PdfWriter<W>, id: usize, file: usize) -> Result<()> {
        let Self { name, bounds, ascent, descent, cap_height, italic_angle, flags, stem } = self;
        let [left, bottom, right, top] = bounds;
        pdf.object(
            id,
            format_args!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} \
                 /FontBBox [{left} {bottom} {right} {top}] /ItalicAngle {italic_angle} \
                 /Ascent {ascent} /Descent {descent} /CapHeight {cap_height} \
                 /StemV {stem} /FontFile2 {file} 0 R >>"
            ),
        )
    }
}

fn postscript_name(font: &FontRef<'_>) -> Result<String> {
    let names = font.name().map_err(invalid)?;
    for platform in [3, 0, 1] {
        for record in names.name_record() {
            if record.name_id() != NameId::POSTSCRIPT_NAME || record.platform_id() != platform {
                continue;
            }
            if !(record.is_unicode() || platform == 1 && record.encoding_id() == 0) {
                continue;
            }
            if record.is_unicode() && !record.length().is_multiple_of(2) {
                return Err(invalid("odd UTF-16 PostScript name length"));
            }
            let name = record.string(names.string_data()).map_err(invalid)?.to_string();
            // OpenType name ID 6: 1..63 printable ASCII characters, excluding
            // PostScript delimiters. '#' is legal there but needs escaping in PDF.
            if !name.is_empty()
                && name.len() <= 63
                && name
                    .bytes()
                    .all(|byte| (33..=126).contains(&byte) && !b"[](){}<>/%".contains(&byte))
            {
                return Ok(name.replace('#', "#23"));
            }
        }
    }
    Err(Error::UnsupportedFont("no usable PostScript font name"))
}

fn invalid(reason: impl std::fmt::Display) -> Error {
    Error::malformed("TrueType font", reason.to_string())
}
