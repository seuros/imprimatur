//! Content and images as JSON.
//!
//! One schema for every caller: the command line's `--json` and the Ruby
//! binding. Offsets are byte offsets into the UTF-8 text they belong to;
//! positions are in points from a page's top-left corner.

#[cfg(test)]
mod tests;

use serde_json::{Map, Value as Json, json};

use super::{
    Block, Bounds, Cell, Columns, Content, Grid, Image, ImageLocation, Look, Mark, Page, Sheet,
    Styled, Validation, Value,
};
use crate::xlsx::column_name;

/// What [`content_with`] adds to the content.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Each formatted cell of a sheet, with its look.
    pub styles: bool,
}

/// The document's content, by format: `pages`, `sheets` or `blocks`.
#[must_use]
pub fn content(content: &Content) -> Map<String, Json> {
    content_with(content, Options::default())
}

/// The document's content, with what `options` add.
#[must_use]
pub fn content_with(content: &Content, options: Options) -> Map<String, Json> {
    let mut object = Map::new();
    object.insert("format".into(), content.format().as_str().into());
    match content {
        Content::Pdf(pages) => object.insert("pages".into(), pages.iter().map(page).collect()),
        Content::Xlsx(sheets) | Content::Ods(sheets) | Content::Xls(sheets) => {
            object.insert("sheets".into(), sheets.iter().map(|it| sheet(it, options)).collect())
        }
        Content::Docx(blocks) => object.insert("blocks".into(), blocks.iter().map(block).collect()),
    };
    object
}

fn page(page: &Page) -> Json {
    let blocks: Vec<Json> = page
        .blocks
        .iter()
        .map(|placed| {
            let mut object = block(&placed.block);
            if let (Some(bounds), Json::Object(fields)) = (placed.bounds, &mut object) {
                fields.insert("bounds".into(), self::bounds(bounds));
            }
            object
        })
        .collect();
    json!({ "page": page.number, "label": page.label, "images": page.images, "blocks": blocks })
}

/// An image's description, its data left out: `format`, `width`, `height`,
/// `bytes`, and where it is: `page` and `bounds`, or `part` and `sheet`.
#[must_use]
pub fn image(image: &Image) -> Map<String, Json> {
    let mut object = Map::new();
    object.insert("format".into(), image.format.extension().into());
    object.insert("width".into(), image.width.into());
    object.insert("height".into(), image.height.into());
    object.insert("bytes".into(), image.data.len().into());
    match &image.location {
        ImageLocation::Page { number, bounds } => {
            object.insert("page".into(), (*number).into());
            if let Some(bounds) = bounds {
                object.insert("bounds".into(), self::bounds(*bounds));
            }
        }
        ImageLocation::Part { name, sheet } => {
            object.insert("part".into(), name.as_str().into());
            if let Some(sheet) = sheet {
                object.insert("sheet".into(), sheet.as_str().into());
            }
        }
    }
    object
}

/// A rectangle, rounded to hundredths of a point.
#[must_use]
pub fn bounds(bounds: Bounds) -> Json {
    let round = |value: f64| (value * 100.0).round() / 100.0;
    json!({
        "left": round(bounds.left),
        "top": round(bounds.top),
        "right": round(bounds.right),
        "bottom": round(bounds.bottom),
    })
}

fn block(block: &Block) -> Json {
    match block {
        Block::Heading { level, text } => {
            json!({ "type": "heading", "level": level, "text": text.text })
        }
        Block::Paragraph { text } => {
            with_marks(json!({ "type": "paragraph", "text": text.text }), &text.marks)
        }
        Block::ListItem { level, ordered, text } => with_marks(
            json!({ "type": "list_item", "level": level, "ordered": ordered, "text": text.text }),
            &text.marks,
        ),
        Block::Table(grid) => json!({ "type": "table", "rows": rows(grid, false) }),
    }
}

fn with_marks(mut object: Json, marks: &[Mark]) -> Json {
    if !marks.is_empty()
        && let Json::Object(fields) = &mut object
    {
        let marks = marks
            .iter()
            .map(|mark| {
                json!({
                    "start": mark.range.start,
                    "end": mark.range.end,
                    "bold": mark.bold,
                    "italic": mark.italic,
                    "link": mark.link,
                })
            })
            .collect();
        fields.insert("marks".into(), marks);
    }
    object
}

fn sheet(sheet: &Sheet, options: Options) -> Json {
    let mut object = Map::new();
    object.insert("name".into(), sheet.name.clone().into());
    object.insert("hidden".into(), sheet.hidden.into());
    object.insert("rows".into(), rows(&sheet.cells, true));
    object.extend(sheet_details(sheet, options));
    Json::Object(object)
}

/// What a sheet has besides its values: `comments`, `columns`,
/// `validations` and, with `options.styles`, `styles`.
#[must_use]
pub fn sheet_details(sheet: &Sheet, options: Options) -> Map<String, Json> {
    let comments: Vec<Json> = sheet
        .comments
        .iter()
        .map(|note| {
            let author = note.author.as_deref();
            json!({ "cell": note.reference(), "author": author, "text": note.text })
        })
        .collect();
    let mut details = Map::new();
    details.insert("comments".into(), comments.into());
    details.insert("columns".into(), sheet.columns.iter().map(columns).collect());
    details.insert("validations".into(), sheet.validations.iter().map(validation).collect());
    if options.styles {
        details.insert("styles".into(), sheet.styles.iter().map(styled).collect());
    }
    details
}

/// Columns by letter: `A`, `XFD`.
fn columns(run: &Columns) -> Json {
    let name = |column: u32| column_name(u16::try_from(column).unwrap_or(u16::MAX));
    let (first, last) = (name(run.first), name(run.last));
    json!({ "first": first, "last": last, "width": run.width, "hidden": run.hidden })
}

fn validation(rule: &Validation) -> Json {
    json!({
        "range": rule.range,
        "type": rule.kind,
        "operator": rule.operator,
        "formula1": rule.formula1,
        "formula2": rule.formula2,
        "allow_blank": rule.allow_blank,
        "dropdown": rule.dropdown,
        "alert": rule.alert,
        "error_title": rule.error_title,
        "error": rule.error,
        "prompt_title": rule.prompt_title,
        "prompt": rule.prompt,
    })
}

fn styled(cell: &Styled) -> Json {
    let Look {
        font,
        size,
        bold,
        italic,
        underline,
        color,
        fill,
        align,
        valign,
        wrap,
        border,
        number_format,
    } = cell.look.as_ref();
    json!({
        "cell": cell.reference(),
        "font": font,
        "size": size,
        "bold": bold,
        "italic": italic,
        "underline": underline,
        "color": color,
        "fill": fill,
        "align": align,
        "valign": valign,
        "wrap": wrap,
        "border": border,
        "number_format": number_format,
    })
}

/// Rows of cells, numbered from 1 in a sheet (where cells are named `B2`)
/// and from 0 in a Word table (where they are numbered by column).
fn rows(grid: &Grid, sheet: bool) -> Json {
    grid.rows()
        .map(|cells| {
            let row = cells.first().map_or(0, |cell| cell.row);
            let cells: Vec<Json> = cells.iter().map(|cell| self::cell(grid, cell, sheet)).collect();
            if sheet {
                json!({ "row": row + 1, "cells": cells })
            } else {
                json!({ "row": row, "cells": cells })
            }
        })
        .collect()
}

fn cell(grid: &Grid, cell: &Cell, sheet: bool) -> Json {
    let text = grid.text(cell);
    let (kind, value) = match cell.value {
        Value::Number(number) => ("number", json!(number)),
        Value::Bool(on) => ("bool", json!(on)),
        Value::Date(date) => ("date", json!(date.to_string().replace(' ', "T"))),
        Value::Error => ("error", json!(text)),
        Value::Formula => ("formula", json!(text)),
        _ => ("text", json!(text)),
    };
    let mut object = Map::new();
    if sheet {
        object.insert("cell".into(), cell.reference().into());
    } else {
        object.insert("column".into(), cell.column.into());
    }
    object.insert("type".into(), kind.into());
    object.insert("value".into(), value);
    if !matches!(kind, "text" | "error" | "formula") {
        object.insert("text".into(), text.into());
    }
    Json::Object(object)
}
