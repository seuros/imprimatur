//! The parts of an `.xlsx` package and the relationships between them.

use std::fmt::Write as _;
use std::io::Write;

use super::cell::{Cell, cell_reference, push_cell_reference};
use super::comment;
use super::styles::Stylesheet;
use super::text::{SharedStrings, escape, needs_preserve};
use super::validation;
use super::worksheet::Worksheet;
use crate::opc;
use crate::xml::XmlWriter;
use crate::zip::ZipWriter;
use crate::{Error, Result};

pub const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

const SPREADSHEET_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml";
pub const VML_TYPE: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";
/// A sheet's own relationships: its comments, then their drawing.
const SHEET_RELATIONSHIPS: [(&str, &str); 2] =
    [("comments", "../comments{n}.xml"), ("vmlDrawing", "../drawings/vmlDrawing{n}.vml")];
const DRAWING_ID: &str = "rId2";

/// Writes the whole package. Each sheet becomes `xl/worksheets/sheetN.xml`,
/// followed by its comments and their drawing if it has any, and is
/// compressed and dropped before the next is built, so only one sheet's XML
/// is in memory at a time; the styles and the shared string table, complete
/// only after every sheet, follow them.
pub fn write<W: Write>(sheets: &[Worksheet], out: W) -> Result<W> {
    if sheets.is_empty() {
        return Err(Error::EmptyWorkbook);
    }
    let has_strings = sheets.iter().any(|sheet| {
        sheet
            .rows()
            .any(|(_, cells)| cells.values().any(|entry| matches!(entry.cell, Cell::Text(_))))
    });

    let mut zip = ZipWriter::new(out);
    zip.add("[Content_Types].xml", content_types(sheets, has_strings).as_bytes())?;
    zip.add(
        "_rels/.rels",
        opc::relationships(&[("officeDocument", "xl/workbook.xml")]).as_bytes(),
    )?;
    zip.add("xl/workbook.xml", workbook(sheets).as_bytes())?;
    zip.add(
        "xl/_rels/workbook.xml.rels",
        workbook_relationships(sheets.len(), has_strings).as_bytes(),
    )?;
    let mut strings = SharedStrings::default();
    let mut styles = Stylesheet::default();
    let mut name = String::new();
    // VML shape ids are unique across the workbook, in blocks of 1024.
    let mut next_block = 1;
    for (number, sheet) in (1..).zip(sheets) {
        let remap = styles.add_sheet(sheet)?;
        set(&mut name, format_args!("xl/worksheets/sheet{number}.xml"));
        zip.add(&name, worksheet(sheet, &remap, &mut strings).as_bytes())?;
        let comments = sheet.comments();
        if comments.is_empty() {
            continue;
        }
        let targets = SHEET_RELATIONSHIPS
            .map(|(kind, target)| (kind, target.replace("{n}", &number.to_string())));
        set(&mut name, format_args!("xl/worksheets/_rels/sheet{number}.xml.rels"));
        zip.add(&name, opc::relationships(&targets).as_bytes())?;
        set(&mut name, format_args!("xl/comments{number}.xml"));
        zip.add(&name, comment::comments_xml(comments).as_bytes())?;
        set(&mut name, format_args!("xl/drawings/vmlDrawing{number}.vml"));
        let drawing = comment::vml(comments, sheet.column_widths(), next_block);
        zip.add(&name, drawing.as_bytes())?;
        next_block += comment::blocks_for(comments.len());
    }
    debug_assert_eq!(has_strings, !strings.is_empty(), "the text scan matches the table");
    zip.add("xl/styles.xml", styles.to_xml().as_bytes())?;
    if has_strings {
        zip.add("xl/sharedStrings.xml", strings.to_xml().as_bytes())?;
    }
    zip.finish()
}

/// One override per part: the workbook, its styles, shared strings, sheets
/// and comments. Drawings, by their `.vml` extension.
fn content_types(sheets: &[Worksheet], has_strings: bool) -> String {
    let mut parts = vec![
        ("/xl/workbook.xml".to_owned(), "sheet.main"),
        ("/xl/styles.xml".to_owned(), "styles"),
    ];
    if has_strings {
        parts.push(("/xl/sharedStrings.xml".to_owned(), "sharedStrings"));
    }
    let mut commented = false;
    for (number, sheet) in (1..).zip(sheets) {
        parts.push((format!("/xl/worksheets/sheet{number}.xml"), "worksheet"));
        if !sheet.comments().is_empty() {
            parts.push((format!("/xl/comments{number}.xml"), "comments"));
            commented = true;
        }
    }
    let overrides: Vec<(String, String)> = parts
        .into_iter()
        .map(|(part, kind)| (part, format!("{SPREADSHEET_TYPE}.{kind}+xml")))
        .collect();
    let defaults: &[(&str, &str)] = if commented { &[("vml", VML_TYPE)] } else { &[] };
    opc::content_types(defaults, &overrides)
}

/// Sheets are `rId1`..`rIdN`, then styles, then shared strings.
fn workbook_relationships(sheet_count: usize, has_strings: bool) -> String {
    let mut targets: Vec<(&str, String)> = (1..=sheet_count)
        .map(|number| ("worksheet", format!("worksheets/sheet{number}.xml")))
        .collect();
    targets.push(("styles", "styles.xml".to_owned()));
    if has_strings {
        targets.push(("sharedStrings", "sharedStrings.xml".to_owned()));
    }
    opc::relationships(&targets)
}

fn workbook(sheets: &[Worksheet]) -> String {
    let mut xml = XmlWriter::new();
    xml.open("workbook", &[("xmlns", MAIN_NS), ("xmlns:r", opc::RELATIONSHIPS)]);
    xml.open("sheets", &[]);
    for (index, sheet) in sheets.iter().enumerate() {
        let number = (index + 1).to_string();
        let id = format!("rId{number}");
        xml.empty("sheet", &[("name", sheet.name()), ("sheetId", &number), ("r:id", &id)]);
    }
    xml.close("sheets");
    // Formulas are written without cached results: have Excel compute them.
    xml.empty("calcPr", &[("fullCalcOnLoad", "1")]);
    xml.close("workbook");
    xml.finish()
}

/// `remap` turns the sheet's own style ids into the workbook's. The cell
/// loop formats into three scratch buffers instead of allocating per cell.
fn worksheet<'a>(sheet: &'a Worksheet, remap: &[u32], strings: &mut SharedStrings<'a>) -> String {
    let mut xml = XmlWriter::new();
    xml.open("worksheet", &[("xmlns", MAIN_NS), ("xmlns:r", opc::RELATIONSHIPS)]);
    xml.empty("dimension", &[("ref", &dimension(sheet))]);
    if sheet.is_right_to_left() {
        xml.open("sheetViews", &[]);
        xml.empty("sheetView", &[("rightToLeft", "1"), ("workbookViewId", "0")]);
        xml.close("sheetViews");
    }
    write_columns(&mut xml, sheet);
    xml.open("sheetData", &[]);
    let (mut reference, mut style, mut value) = (String::new(), String::new(), String::new());
    for (row, cells) in sheet.rows() {
        set(&mut value, u64::from(row) + 1);
        xml.open("row", &[("r", &value)]);
        for (&column, entry) in cells {
            reference.clear();
            push_cell_reference(&mut reference, row, column);
            // Every style id a cell holds came from the sheet's own table.
            let id = remap.get(entry.style as usize).copied().unwrap_or_default();
            set(&mut style, id);
            let styled = id != 0;
            let (kind, text) = match &entry.cell {
                Cell::Text(text) => {
                    set(&mut value, strings.intern(text));
                    (Some("s"), None)
                }
                Cell::Number(number) => {
                    set(&mut value, number);
                    (None, None)
                }
                Cell::DateTime(date) => {
                    set(&mut value, date.serial());
                    (None, None)
                }
                Cell::Bool(flag) => {
                    set(&mut value, u8::from(*flag));
                    (Some("b"), None)
                }
                Cell::Formula(formula) => (None, Some(formula)),
            };
            let attrs =
                [("r", reference.as_str()), ("s", style.as_str()), ("t", kind.unwrap_or(""))];
            let attrs: &[(&str, &str)] = match (styled, kind.is_some()) {
                (true, true) => &attrs,
                (true, false) => &attrs[..2],
                (false, true) => &[attrs[0], attrs[2]],
                (false, false) => &attrs[..1],
            };
            xml.open("c", attrs);
            match text {
                Some(formula) => {
                    let preserve: &[(&str, &str)] =
                        if needs_preserve(formula) { &[("xml:space", "preserve")] } else { &[] };
                    xml.element("f", preserve, &escape(formula));
                }
                None => xml.element("v", &[], &value),
            }
            xml.close("c");
        }
        xml.close("row");
    }
    xml.close("sheetData");
    validation::write_all(&mut xml, sheet.validations());
    if !sheet.comments().is_empty() {
        xml.empty("legacyDrawing", &[("r:id", DRAWING_ID)]);
    }
    xml.close("worksheet");
    xml.finish()
}

/// Replaces `buffer` with `value`'s text, reusing its allocation.
fn set(buffer: &mut String, value: impl std::fmt::Display) {
    buffer.clear();
    // Writing to a String cannot fail.
    let _ = write!(buffer, "{value}");
}

/// `<cols>`: one `<col>` per run of adjacent columns with the same width.
fn write_columns(xml: &mut XmlWriter, sheet: &Worksheet) {
    let widths = sheet.column_widths();
    if widths.is_empty() {
        return;
    }
    let mut runs: Vec<(u16, u16, f64)> = Vec::new();
    for (&column, &width) in widths {
        match runs.last_mut() {
            Some((_, last, run_width))
                if *last + 1 == column && run_width.to_bits() == width.to_bits() =>
            {
                *last = column;
            }
            _ => runs.push((column, column, width)),
        }
    }
    xml.open("cols", &[]);
    for (first, last, width) in runs {
        // <col> numbers columns from 1.
        let (min, max) = ((first + 1).to_string(), (last + 1).to_string());
        let width = width.to_string();
        xml.empty("col", &[("min", &min), ("max", &max), ("width", &width), ("customWidth", "1")]);
    }
    xml.close("cols");
}

/// The used range, `A1` for an empty sheet as Excel writes it.
fn dimension(sheet: &Worksheet) -> String {
    let mut bounds: Option<(u32, u16, u32, u16)> = None;
    for (row, cells) in sheet.rows() {
        let (Some((&first, _)), Some((&last, _))) =
            (cells.first_key_value(), cells.last_key_value())
        else {
            continue;
        };
        bounds = Some(match bounds {
            None => (row, first, row, last),
            Some((top, left, _, right)) => (top, left.min(first), row, right.max(last)),
        });
    }
    let Some((top, left, bottom, right)) = bounds else {
        return "A1".to_owned();
    };
    let mut range = cell_reference(top, left);
    if (top, left) != (bottom, right) {
        // Writing to a String cannot fail.
        let _ = write!(range, ":{}", cell_reference(bottom, right));
    }
    range
}
