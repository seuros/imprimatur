//! A complete TrueType font as a horizontal Type 0 / `CIDFontType2` resource
//! (ISO 32000-1, 9.7 and 9.10). Document font selection and layout are separate.

mod descriptor;
mod program;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::Write;

use super::objects::{Num, PdfWriter};
use crate::font::TrueTypeFont;
use crate::{Error, Result};
use descriptor::Descriptor;

/// Objects written in order: Type 0 font, descendant, descriptor, font file,
/// CID-to-glyph map, `ToUnicode` character map.
pub const OBJECTS: usize = 6;

/// Collects glyph uses before writing the font resource once for all its pages.
/// CIDs are allocated per (glyph, original text), not per glyph alone: distinct
/// characters can share an outline, and a ligature can represent several.
pub struct EmbeddedFont<'a> {
    font: TrueTypeFont<'a>,
    descriptor: Descriptor,
    program: Vec<u8>,
    /// CID 1 onwards, with unscaled advances; CID 0 is reserved for .notdef.
    glyphs: Vec<(u16, u16)>,
    codes: BTreeMap<(u16, String), u16>,
}

impl<'a> EmbeddedFont<'a> {
    /// Reads the metadata needed for a PDF descriptor and checks embedding rights.
    pub fn new(font: TrueTypeFont<'a>) -> Result<Self> {
        let descriptor = Descriptor::read(&font)?;
        let program = program::for_pdf(&font)?;
        Ok(Self { font, descriptor, program, glyphs: Vec::new(), codes: BTreeMap::new() })
    }

    /// The two big-endian bytes to put in a PDF text string for this glyph use.
    /// `text` is its original Unicode sequence, in logical order. Empty text
    /// omits its `ToUnicode` entry, for glyphs with no text of their own.
    /// This does not shape text or infer glyphs from characters.
    pub fn encode_glyph(&mut self, glyph: u16, text: &str) -> Result<[u8; 2]> {
        // A ToUnicode destination string may contain at most 512 bytes.
        (text.encode_utf16().take(257).count() <= 256)
            .ok_or_else(|| invalid("Unicode mapping exceeds 256 UTF-16 code units"))?;
        let width = self
            .font
            .advance_width(glyph)
            .filter(|_| glyph != 0)
            .ok_or_else(|| invalid("glyph is missing or outside the font's glyph range"))?;
        let key = (glyph, text.to_owned());
        if let Some(code) = self.codes.get(&key) {
            return Ok(code.to_be_bytes());
        }
        let code = u16::try_from(self.glyphs.len() + 1)
            .map_err(|_| invalid("more than 65535 distinct glyph mappings"))?;
        self.glyphs.push((glyph, width));
        self.codes.insert(key, code);
        Ok(code.to_be_bytes())
    }

    /// Writes six consecutive objects, starting at `first`, which is the font
    /// reference for a page's resources. Collect every glyph before calling this.
    /// Every glyph is embedded with its original ID and outline. The font's cmap
    /// is replaced by PDF mappings; its table directory and checksums are rebuilt.
    pub fn write<W: Write>(&self, pdf: &mut PdfWriter<W>, first: usize) -> Result<()> {
        let (descendant, descriptor, file, mapping, unicode) =
            (first + 1, first + 2, first + 3, first + 4, first + 5);
        let name = &self.descriptor.name;
        pdf.object(
            first,
            format_args!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
                 /DescendantFonts [{descendant} 0 R] /ToUnicode {unicode} 0 R >>"
            ),
        )?;
        let scale = 1000.0 / f64::from(self.font.metrics().units_per_em);
        let default_width = Num(f64::from(self.font.advance_width(0).unwrap_or(0)) * scale);
        let widths = fmt::from_fn(|f| {
            // Keep both the outer W array and each inner width array below the
            // PDF 1.7 implementation limit of 8191 elements (Annex C).
            for (index, glyphs) in self.glyphs.chunks(256).enumerate() {
                if index > 0 {
                    f.write_str(" ")?;
                }
                write!(f, "{} [", 1 + index * 256)?;
                for &(_, width) in glyphs {
                    write!(f, "{} ", Num(f64::from(width) * scale))?;
                }
                f.write_str("]")?;
            }
            Ok(())
        });
        pdf.object(
            descendant,
            format_args!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {descriptor} 0 R /DW {default_width} /W [{widths}] \
                 /CIDToGIDMap {mapping} 0 R >>"
            ),
        )?;
        self.descriptor.write(pdf, descriptor, file)?;
        pdf.stream(file, &self.program, &[("Length1", &self.program.len())])?;
        let mut gids = Vec::with_capacity(2 * (self.glyphs.len() + 1));
        gids.extend_from_slice(&[0, 0]);
        for &(glyph, _) in &self.glyphs {
            gids.extend_from_slice(&glyph.to_be_bytes());
        }
        pdf.stream(mapping, &gids, &[])?;
        pdf.stream(unicode, self.to_unicode().as_bytes(), &[])
    }

    fn to_unicode(&self) -> String {
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Imprimatur-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let mappings: Vec<_> =
            self.codes.iter().filter(|((_, text), _)| !text.is_empty()).collect();
        // CMap mapping operators accept at most 100 entries per block.
        for chunk in mappings.chunks(100) {
            // Writing to a String cannot fail.
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for ((_, text), code) in chunk {
                let _ = write!(cmap, "<{code:04X}> <");
                for unit in text.encode_utf16() {
                    let _ = write!(cmap, "{unit:04X}");
                }
                cmap.push_str(">\n");
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        cmap
    }
}

fn invalid(reason: &str) -> Error {
    Error::malformed("PDF font", reason)
}
