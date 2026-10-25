//! Static TrueType font data, Unicode glyph lookup and unscaled metrics.
//!
//! This does not select or embed fonts in documents, shape text, or perform
//! bidirectional layout. A character's nominal glyph is not necessarily the
//! glyph used when shaping a script such as Arabic.

#[cfg(test)]
mod tests;

use std::fmt;

use read_fonts::{
    FontRead, FontRef, MinByteRange, TableProvider,
    tables::{
        cmap::{Cmap, CmapSubtable, PlatformId},
        hmtx::Hmtx,
    },
    types::{GlyphId, Tag, Version16Dot16},
};

use crate::{Error, Result};

/// Horizontal line metrics in font design units, taken from `head` and `hhea`.
///
/// Multiply a metric by the point size and divide by `units_per_em` to convert
/// it to points. These are the raw `hhea` metrics, not an OS/2 line-height policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascender: i16,
    /// Usually negative: the position below the baseline.
    pub descender: i16,
    pub line_gap: i16,
}

/// A borrowed, standalone static TrueType font with a Unicode character map.
///
/// Loading checks table bounds and the data needed for glyph lookup and metrics,
/// including the `loca` offsets into `glyf`. It is not a full font sanitizer:
/// checksums, outline programs and shaping tables are not validated.
#[derive(Clone)]
pub struct TrueTypeFont<'a> {
    data: &'a [u8],
    cmap: CmapSubtable<'a>,
    hmtx: Hmtx<'a>,
    metrics: FontMetrics,
    glyph_count: u16,
}

impl fmt::Debug for TrueTypeFont<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrueTypeFont")
            .field("data_len", &self.data.len())
            .field("metrics", &self.metrics)
            .field("glyph_count", &self.glyph_count)
            .finish_non_exhaustive()
    }
}

impl<'a> TrueTypeFont<'a> {
    /// Load font data without copying it or accessing system fonts.
    ///
    /// # Errors
    /// [`Error::UnsupportedFont`] for collections, WOFF, CFF, variable fonts or
    /// fonts without a supported Unicode cmap. [`Error::Malformed`] for invalid
    /// headers, missing required tables or inconsistent metrics/outline offsets.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self> {
        match data.get(..4) {
            Some(b"\0\x01\0\0" | b"true") => {}
            Some(b"ttcf") => return Err(Error::UnsupportedFont("font collections")),
            Some(b"wOFF" | b"wOF2") => return Err(Error::UnsupportedFont("WOFF/WOFF2")),
            Some(b"OTTO") => return Err(Error::UnsupportedFont("CFF outlines")),
            _ => return Err(invalid("invalid or truncated sfnt header")),
        }
        let font = FontRef::new(data).map_err(invalid)?;
        for record in font.table_directory().table_records() {
            if u64::from(record.offset()) + u64::from(record.length()) > data.len() as u64 {
                return Err(invalid(format!(
                    "{} table extends beyond the font data",
                    record.tag()
                )));
            }
            match record.tag() {
                tag if tag == Tag::new(b"fvar") || tag == Tag::new(b"gvar") => {
                    return Err(Error::UnsupportedFont("variable fonts"));
                }
                tag if tag == Tag::new(b"CFF ") || tag == Tag::new(b"CFF2") => {
                    return Err(Error::UnsupportedFont("CFF outlines"));
                }
                _ => {}
            }
        }

        let head = font.head().map_err(invalid)?;
        if head.magic_number() != 0x5F0F_3CF5 || !(16..=16384).contains(&head.units_per_em()) {
            return Err(invalid("invalid head magic number or units per em"));
        }
        let maxp = font.maxp().map_err(invalid)?;
        let glyph_count = maxp.num_glyphs();
        if maxp.version() != Version16Dot16::VERSION_1_0
            || maxp.max_component_depth().is_none()
            || glyph_count == 0
        {
            return Err(invalid("expected a complete TrueType maxp table with at least one glyph"));
        }
        let hhea = font.hhea().map_err(invalid)?;
        let count = hhea.number_of_h_metrics();
        if count == 0 || count > glyph_count {
            return Err(invalid("hhea metric count is outside the glyph range"));
        }
        let hmtx = font.hmtx().map_err(invalid)?;
        if hmtx.h_metrics().len() != usize::from(count)
            || hmtx.left_side_bearings().len() < usize::from(glyph_count - count)
        {
            return Err(invalid("truncated hmtx table"));
        }
        validate_outlines(&font, head.index_to_loc_format(), glyph_count)?;
        let cmap = font.cmap().map_err(invalid)?;
        if cmap.version() != 0 || cmap.encoding_records().len() != usize::from(cmap.num_tables()) {
            return Err(invalid("invalid or truncated cmap header"));
        }
        Ok(Self {
            data,
            cmap: unicode_cmap(&cmap)?,
            hmtx,
            metrics: FontMetrics {
                units_per_em: head.units_per_em(),
                ascender: hhea.ascender().into(),
                descender: hhea.descender().into(),
                line_gap: hhea.line_gap().into(),
            },
            glyph_count,
        })
    }

    /// The original font bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Unscaled horizontal line metrics.
    #[must_use]
    pub const fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// The number of glyphs, including glyph zero (`.notdef`).
    #[must_use]
    pub const fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// The nominal glyph for a Unicode character, without shaping or fallback.
    ///
    /// Returns `None` for missing characters, mappings to `.notdef`, or mappings
    /// outside the font's glyph range. Symbol and legacy cmaps are never used as
    /// Unicode mappings; full-repertoire Unicode cmaps take priority over BMP ones.
    #[must_use]
    pub fn glyph_id(&self, character: char) -> Option<u16> {
        let glyph = self.cmap.map_codepoint(character)?.to_u32();
        u16::try_from(glyph).ok().filter(|&id| id != 0 && id < self.glyph_count)
    }

    /// The horizontal advance in font design units, without kerning or shaping.
    ///
    /// Glyph zero is allowed; an out-of-range glyph returns `None`. A valid
    /// zero-width glyph returns `Some(0)`.
    #[must_use]
    pub fn advance_width(&self, glyph_id: u16) -> Option<u16> {
        if glyph_id >= self.glyph_count {
            return None;
        }
        self.hmtx.advance(GlyphId::new(u32::from(glyph_id)))
    }
}

fn validate_outlines(font: &FontRef<'_>, loca_format: i16, glyph_count: u16) -> Result<()> {
    if !matches!(loca_format, 0 | 1) {
        return Err(invalid("invalid head indexToLocFormat"));
    }
    let loca = font.loca(loca_format == 1).map_err(invalid)?;
    let glyf = font.glyf().map_err(invalid)?;
    if loca.len() != usize::from(glyph_count)
        || !loca.all_offsets_are_ascending()
        || loca
            .get_raw(usize::from(glyph_count))
            .is_none_or(|end| u64::from(end) > glyf.offset_data().len() as u64)
    {
        return Err(invalid("invalid loca offsets or truncated glyf table"));
    }
    Ok(())
}

fn unicode_cmap<'a>(cmap: &Cmap<'a>) -> Result<CmapSubtable<'a>> {
    // Fontations' best_subtable also accepts symbol and MacRoman maps. Only
    // Unicode encodings belong here; prefer full repertoire over BMP mappings.
    let encodings = [
        (PlatformId::Windows, 10),
        (PlatformId::Unicode, 6),
        (PlatformId::Unicode, 4),
        (PlatformId::Windows, 1),
        (PlatformId::Unicode, 3),
        (PlatformId::Unicode, 2),
        (PlatformId::Unicode, 1),
        (PlatformId::Unicode, 0),
    ];
    for (platform, encoding) in encodings {
        for record in cmap.encoding_records() {
            if record.platform_id() != platform || record.encoding_id() != encoding {
                continue;
            }
            let subtable = record.subtable(cmap.offset_data()).map_err(invalid)?;
            let length = match &subtable {
                CmapSubtable::Format0(table) => u32::from(table.length()),
                CmapSubtable::Format4(table) => u32::from(table.length()),
                CmapSubtable::Format6(table) => u32::from(table.length()),
                CmapSubtable::Format10(table) => table.length(),
                CmapSubtable::Format12(table) => table.length(),
                CmapSubtable::Format13(table) => table.length(),
                _ => continue,
            };
            let length = usize::try_from(length).map_err(|_| invalid("cmap subtable too large"))?;
            let data = subtable
                .offset_data()
                .slice(0..length)
                .ok_or_else(|| invalid("truncated cmap subtable"))?;
            // Constrain lazy array reads to this subtable's declared extent.
            let subtable = CmapSubtable::read(data).map_err(invalid)?;
            if subtable.min_byte_range().end > length {
                return Err(invalid("truncated cmap arrays"));
            }
            return Ok(subtable);
        }
    }
    Err(Error::UnsupportedFont("no supported Unicode cmap"))
}

/// `Malformed`, categorized as TrueType font data: parsing, embedding and PDF
/// resource errors ([`crate::pdf::truetype`]) all point at the same category.
pub(crate) fn invalid(reason: impl std::fmt::Display) -> Error {
    Error::malformed("TrueType font", reason.to_string())
}
