//! `xl/styles.xml`: every distinct format in the workbook, split into the
//! number formats, fonts, fills and borders it refers to.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use super::format::{Border, DEFAULT_FONT, Font, Format};
use super::package::MAIN_NS;
use super::worksheet::{MAX_FORMATS, Worksheet};
use crate::style::{Align, Color};
use crate::xml::XmlWriter;
use crate::{Error, Result};

/// The first id Excel leaves free for custom number formats.
const FIRST_CUSTOM_NUMBER_FORMAT: u32 = 164;

/// Distinct values in order of first use, numbered from a starting id.
/// There are at most [`MAX_FORMATS`] of them, so ids stay small.
struct Table<T> {
    ids: HashMap<T, u32>,
    order: Vec<T>,
    next: u32,
}

impl<T: Copy + Eq + Hash> Table<T> {
    fn new(first: u32) -> Self {
        Self { ids: HashMap::new(), order: Vec::new(), next: first }
    }

    fn id(&mut self, value: &T) -> u32 {
        if let Some(&id) = self.ids.get(value) {
            return id;
        }
        let id = self.next;
        self.next += 1;
        self.order.push(*value);
        self.ids.insert(*value, id);
        id
    }
}

/// The workbook's cell formats; cell style `n` is `formats[n - 1]`, 0 the default.
#[derive(Default)]
pub struct Stylesheet {
    /// Shared with the sheets that use them: merging copies no format.
    ids: HashMap<Arc<Format>, u32>,
    formats: Vec<Arc<Format>>,
}

impl Stylesheet {
    /// Merges a sheet's formats in; returns the workbook style for each of
    /// the sheet's own style ids (index 0, the default, stays 0).
    pub fn add_sheet(&mut self, sheet: &Worksheet) -> Result<Vec<u32>> {
        let mut remap = vec![0];
        for format in sheet.formats() {
            if let Some(&id) = self.ids.get(format) {
                remap.push(id);
                continue;
            }
            (self.formats.len() < MAX_FORMATS).ok_or(Error::TooManyFormats)?;
            self.formats.push(Arc::clone(format));
            let id = u32::try_from(self.formats.len()).map_err(|_| Error::TooManyFormats)?;
            self.ids.insert(Arc::clone(format), id);
            remap.push(id);
        }
        Ok(remap)
    }

    pub fn to_xml(&self) -> String {
        let mut number_formats = Table::<&str>::new(FIRST_CUSTOM_NUMBER_FORMAT);
        let mut fonts = Table::<&Font>::new(1);
        let mut fills = Table::<Color>::new(2);
        let mut borders = Table::<Border>::new(1);
        let xfs: Vec<[u32; 4]> = self
            .formats
            .iter()
            .map(|format| {
                let number = match format.number_code.as_deref() {
                    None | Some("General") => 0,
                    Some(code) => number_formats.id(&code),
                };
                let font = if format.font == Font::default() { 0 } else { fonts.id(&&format.font) };
                let fill = format.fill.map_or(0, |color| fills.id(&color));
                let border = format.border.map_or(0, |border| borders.id(&border));
                [number, font, fill, border]
            })
            .collect();

        let mut xml = XmlWriter::new();
        xml.open("styleSheet", &[("xmlns", MAIN_NS)]);
        if !number_formats.order.is_empty() {
            let count = number_formats.order.len().to_string();
            xml.open("numFmts", &[("count", &count)]);
            for (offset, code) in (0..).zip(&number_formats.order) {
                let id = (FIRST_CUSTOM_NUMBER_FORMAT + offset).to_string();
                xml.empty("numFmt", &[("numFmtId", &id), ("formatCode", code)]);
            }
            xml.close("numFmts");
        }
        write_fonts(&mut xml, &fonts.order);
        write_fills(&mut xml, &fills.order);
        write_borders(&mut xml, &borders.order);
        xml.open("cellStyleXfs", &[("count", "1")]);
        xml.empty("xf", &[("numFmtId", "0"), ("fontId", "0"), ("fillId", "0"), ("borderId", "0")]);
        xml.close("cellStyleXfs");
        let count = (xfs.len() + 1).to_string();
        xml.open("cellXfs", &[("count", &count)]);
        xml.empty(
            "xf",
            &[
                ("numFmtId", "0"),
                ("fontId", "0"),
                ("fillId", "0"),
                ("borderId", "0"),
                ("xfId", "0"),
            ],
        );
        for (format, ids) in self.formats.iter().zip(&xfs) {
            write_xf(&mut xml, format, *ids);
        }
        xml.close("cellXfs");
        xml.open("cellStyles", &[("count", "1")]);
        xml.empty("cellStyle", &[("name", "Normal"), ("xfId", "0"), ("builtinId", "0")]);
        xml.close("cellStyles");
        xml.close("styleSheet");
        xml.finish()
    }
}

fn write_fonts(xml: &mut XmlWriter, fonts: &[&Font]) {
    let count = (fonts.len() + 1).to_string();
    xml.open("fonts", &[("count", &count)]);
    let default = Font::default();
    for font in std::iter::once(&default).chain(fonts.iter().copied()) {
        xml.open("font", &[]);
        for (on, tag) in [(font.bold, "b"), (font.italic, "i"), (font.underline, "u")] {
            if on {
                xml.empty(tag, &[]);
            }
        }
        xml.empty("sz", &[("val", &font.size())]);
        if let Some(color) = font.color {
            xml.empty("color", &[("rgb", &color.argb())]);
        }
        xml.empty("name", &[("val", font.name())]);
        // The family is a fallback hint; only Calibri's is known (2, swiss).
        if font.name() == DEFAULT_FONT {
            xml.empty("family", &[("val", "2")]);
        }
        xml.close("font");
    }
    xml.close("fonts");
}

/// Excel reserves the first two fills: none and gray125.
fn write_fills(xml: &mut XmlWriter, fills: &[Color]) {
    let count = (fills.len() + 2).to_string();
    xml.open("fills", &[("count", &count)]);
    for pattern in ["none", "gray125"] {
        xml.open("fill", &[]);
        xml.empty("patternFill", &[("patternType", pattern)]);
        xml.close("fill");
    }
    for color in fills {
        xml.open("fill", &[]);
        xml.open("patternFill", &[("patternType", "solid")]);
        xml.empty("fgColor", &[("rgb", &color.argb())]);
        xml.empty("bgColor", &[("indexed", "64")]);
        xml.close("patternFill");
        xml.close("fill");
    }
    xml.close("fills");
}

fn write_borders(xml: &mut XmlWriter, borders: &[Border]) {
    let count = (borders.len() + 1).to_string();
    xml.open("borders", &[("count", &count)]);
    xml.open("border", &[]);
    for side in ["left", "right", "top", "bottom", "diagonal"] {
        xml.empty(side, &[]);
    }
    xml.close("border");
    for border in borders {
        xml.open("border", &[]);
        for side in ["left", "right", "top", "bottom"] {
            xml.open(side, &[("style", border.as_str())]);
            xml.empty("color", &[("auto", "1")]);
            xml.close(side);
        }
        xml.empty("diagonal", &[]);
        xml.close("border");
    }
    xml.close("borders");
}

fn write_xf(xml: &mut XmlWriter, format: &Format, [number, font, fill, border]: [u32; 4]) {
    let ids = [number, font, fill, border].map(|id| id.to_string());
    let mut attrs = vec![
        ("numFmtId", ids[0].as_str()),
        ("fontId", ids[1].as_str()),
        ("fillId", ids[2].as_str()),
        ("borderId", ids[3].as_str()),
        ("xfId", "0"),
    ];
    for (applies, name) in [
        (number != 0, "applyNumberFormat"),
        (font != 0, "applyFont"),
        (fill != 0, "applyFill"),
        (border != 0, "applyBorder"),
        (format.has_alignment(), "applyAlignment"),
    ] {
        if applies {
            attrs.push((name, "1"));
        }
    }
    if !format.has_alignment() {
        xml.empty("xf", &attrs);
        return;
    }
    xml.open("xf", &attrs);
    let mut alignment = Vec::new();
    if let Some(align) = format.align {
        alignment.push(("horizontal", Align::as_str(align)));
    }
    if let Some(valign) = format.valign {
        alignment.push(("vertical", valign.as_str()));
    }
    if let Some(direction) = format.direction {
        alignment.push(("readingOrder", direction.reading_order()));
    }
    if format.wrap {
        alignment.push(("wrapText", "1"));
    }
    xml.empty("alignment", &alignment);
    xml.close("xf");
}
