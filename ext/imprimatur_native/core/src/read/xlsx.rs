//! XLSX: each sheet's cells, with their values and their text as Excel
//! shows it (dates as dates, percentages as percentages, 15 significant
//! digits), formulas without a cached result as `=FORMULA`, then the sheet's
//! comments.

mod numbers;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use super::Budget;
use super::content::{Grid, Note, Sheet, Value};
use super::package::Package;
use crate::Result;
use crate::xlsx::parse_cell_reference;
use crate::xlsx::text::unescape as unescape_xstring;
use crate::xml::reader::{Event, XmlReader};
use numbers::{Shown, push_number, styles};

/// A cell's `t` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Shared,
    Inline,
    Text,
    Bool,
    Error,
    Iso,
}

/// A sheet as the workbook lists it.
struct Listed {
    name: String,
    part: String,
    hidden: bool,
}

/// The shared string table: every string in one buffer.
#[derive(Default)]
struct Strings {
    text: String,
    ends: Vec<usize>,
}

impl Strings {
    fn get(&self, index: usize) -> Option<&str> {
        let end = *self.ends.get(index)?;
        let start = index.checked_sub(1).map_or(0, |previous| self.ends[previous]);
        Some(&self.text[start..end])
    }
}

/// Where a cell's text events go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Into {
    Nowhere,
    Value,
    Formula,
    Inline,
}

pub fn content(package: &Package<'_>, budget: &Budget<'_>) -> Result<Vec<Sheet>> {
    let main = package.main();
    let text = package.main_text()?;
    let relationships = package.relationships(main)?;
    let (listed, date1904) = workbook(&text, &relationships, budget)?;
    let strings = package
        .parse_related(&relationships, "sharedStrings", |text| shared_strings(text, budget))?;
    let shown = package.parse_related(&relationships, "styles", |text| styles(text, budget))?;
    let book = Book { strings: &strings, shown: &shown, date1904 };
    let mut sheets = Vec::with_capacity(listed.len());
    let mut scratch = String::new();
    for sheet in listed {
        budget.check()?;
        let Some(text) = package.text(&sheet.part)? else { continue };
        let cells = cells(&text, &book, &mut scratch, budget)?;
        let comments = match package.related(&sheet.part, "comments")? {
            Some(part) => package
                .text(&part)?
                .map_or_else(|| Ok(Vec::new()), |text| comments(&text, budget))?,
            None => Vec::new(),
        };
        sheets.push(Sheet { name: sheet.name, hidden: sheet.hidden, cells, comments });
    }
    Ok(sheets)
}

/// The sheets in order, with their parts, and whether dates count from 1904.
fn workbook(
    text: &str,
    relationships: &[super::package::Relationship],
    budget: &Budget<'_>,
) -> Result<(Vec<Listed>, bool)> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let (mut sheets, mut date1904) = (Vec::new(), false);
    while let Some(event) = xml.next()? {
        let Event::Start(tag) = event else { continue };
        match tag.name() {
            "workbookPr" => {
                date1904 = tag.attr("date1904").is_some_and(|on| on == "1" || on == "true");
            }
            "sheet" => {
                let (Some(name), Some(id)) = (tag.attr("name"), tag.attr("id")) else { continue };
                let Some(relationship) = relationships.iter().find(|r| r.id == id) else {
                    continue;
                };
                let hidden = tag.attr("state").is_some_and(|state| state != "visible");
                sheets.push(Listed {
                    name: name.into_owned(),
                    part: relationship.target.clone(),
                    hidden,
                });
            }
            _ => {}
        }
    }
    Ok((sheets, date1904))
}

fn shared_strings(text: &str, budget: &Budget<'_>) -> Result<Strings> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let mut strings = Strings::default();
    // Phonetic runs (rPh) annotate East Asian text; they are not its text.
    let (mut in_text, mut phonetic) = (false, 0_usize);
    while let Some(event) = xml.next()? {
        budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "t" => in_text = phonetic == 0,
                "rPh" => phonetic += 1,
                _ => {}
            },
            Event::Text(text) if in_text => strings.text.push_str(&unescape_xstring(&text)),
            Event::End("t") => in_text = false,
            Event::End("rPh") => phonetic = phonetic.saturating_sub(1),
            Event::End("si") => strings.ends.push(strings.text.len()),
            Event::Text(_) | Event::End(_) => {}
        }
    }
    Ok(strings)
}

struct Book<'a> {
    strings: &'a Strings,
    shown: &'a [Shown],
    date1904: bool,
}

/// A cell's position, kind and format, while its children are read.
struct Head {
    row: u32,
    column: u32,
    kind: Kind,
    style: usize,
}

fn cells(text: &str, book: &Book<'_>, scratch: &mut String, budget: &Budget<'_>) -> Result<Grid> {
    let mut grid = Grid::default();
    let mut xml = XmlReader::new(text, budget.max_depth());
    let (mut row, mut next_row, mut next_column) = (0_u32, 0_u32, 0_u32);
    let (mut value, mut formula, mut inline) = (String::new(), String::new(), String::new());
    let (mut head, mut into) = (None, Into::Nowhere);
    while let Some(event) = xml.next()? {
        budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "row" => {
                    row = tag
                        .attr("r")
                        .and_then(|r| r.parse::<u32>().ok())
                        .map_or(next_row, |r| r.saturating_sub(1));
                    next_column = 0;
                }
                "c" => {
                    let (at_row, column) = tag
                        .attr("r")
                        .and_then(|reference| parse_cell_reference(&reference).ok())
                        .map_or((row, next_column), |(row, column)| (row, u32::from(column)));
                    let kind = match tag.attr("t").as_deref() {
                        Some("s") => Kind::Shared,
                        Some("inlineStr") => Kind::Inline,
                        Some("str") => Kind::Text,
                        Some("b") => Kind::Bool,
                        Some("e") => Kind::Error,
                        Some("d") => Kind::Iso,
                        _ => Kind::Number,
                    };
                    let style = tag.attr("s").and_then(|s| s.parse().ok()).unwrap_or(0);
                    head = Some(Head { row: at_row, column, kind, style });
                    value.clear();
                    formula.clear();
                    inline.clear();
                }
                "v" => into = Into::Value,
                "f" => into = Into::Formula,
                "t" if head.as_ref().is_some_and(|head| head.kind == Kind::Inline) => {
                    into = Into::Inline;
                }
                "rPh" => xml.skip()?,
                _ => {}
            },
            Event::Text(text) => match into {
                Into::Value => value.push_str(&text),
                Into::Formula => formula.push_str(&text),
                Into::Inline => inline.push_str(&unescape_xstring(&text)),
                Into::Nowhere => {}
            },
            Event::End("v" | "f" | "t") => into = Into::Nowhere,
            Event::End("c") => {
                if let Some(head) = head.take() {
                    write_cell(&mut grid, &head, book, [&value, &formula, &inline], scratch);
                    next_column = head.column + 1;
                }
            }
            Event::End("row") => next_row = row + 1,
            Event::End(_) => {}
        }
    }
    Ok(grid.finish())
}

fn write_cell(
    grid: &mut Grid,
    head: &Head,
    book: &Book<'_>,
    [value, formula, inline]: [&str; 3],
    scratch: &mut String,
) {
    let out = grid.buffer();
    let kind = match head.kind {
        Kind::Shared => {
            if let Some(text) = value.trim().parse().ok().and_then(|index| book.strings.get(index))
            {
                out.push_str(text);
            }
            Value::Text
        }
        Kind::Inline => {
            out.push_str(inline);
            Value::Text
        }
        Kind::Text | Kind::Iso => {
            out.push_str(&unescape_xstring(value));
            Value::Text
        }
        Kind::Error => {
            out.push_str(&unescape_xstring(value));
            Value::Error
        }
        Kind::Bool => {
            let on = value.trim() == "1";
            out.push_str(if on { "TRUE" } else { "FALSE" });
            Value::Bool(on)
        }
        Kind::Number => match value.trim().parse::<f64>() {
            Ok(number) => {
                let shown = book.shown.get(head.style).copied().unwrap_or(Shown::General);
                push_number(out, number, shown, book.date1904, scratch)
            }
            Err(_) if value.trim().is_empty() && !formula.trim().is_empty() => {
                out.push('=');
                out.push_str(formula.trim());
                Value::Formula
            }
            Err(_) => {
                out.push_str(value);
                Value::Text
            }
        },
    };
    grid.end_cell(head.row, head.column, kind);
}

/// The sheet's comments.
fn comments(text: &str, budget: &Budget<'_>) -> Result<Vec<Note>> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let (mut authors, mut author, mut body) =
        (Vec::<Arc<str>>::new(), String::new(), String::new());
    let (mut in_author, mut in_text, mut position, mut author_id) = (false, false, None, 0);
    let mut notes = Vec::new();
    while let Some(event) = xml.next()? {
        budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "author" => in_author = true,
                "comment" => {
                    position = parse_cell_reference(&tag.attr("ref").unwrap_or_default()).ok();
                    author_id =
                        tag.attr("authorId").and_then(|id| id.parse().ok()).unwrap_or(usize::MAX);
                    body.clear();
                }
                "t" => in_text = true,
                "rPh" => xml.skip()?,
                _ => {}
            },
            Event::Text(text) if in_author => author.push_str(&text),
            Event::Text(text) if in_text => body.push_str(&unescape_xstring(&text)),
            Event::End("author") => {
                in_author = false;
                authors.push(Arc::from(author.as_str()));
                author.clear();
            }
            Event::End("t") => in_text = false,
            // A comment on no cell has nowhere to be shown.
            Event::End("comment") => {
                if let Some((row, column)) = position.take() {
                    notes.push(Note {
                        row,
                        column: u32::from(column),
                        author: authors.get(author_id).filter(|name| !name.is_empty()).cloned(),
                        text: body.trim().to_owned(),
                    });
                }
            }
            Event::Text(_) | Event::End(_) => {}
        }
    }
    Ok(notes)
}

/// Which sheet shows each image: `(media part, sheet name)`, through the
/// sheets' drawings.
pub fn image_sheets(package: &Package<'_>, budget: &Budget<'_>) -> Result<Vec<(String, String)>> {
    let main = package.main();
    let relationships = package.relationships(main)?;
    let (listed, _) = workbook(&package.main_text()?, &relationships, budget)?;
    let mut shown = Vec::new();
    for sheet in listed {
        for drawing in package.relationships(&sheet.part)?.iter().filter(|r| r.kind == "drawing") {
            for image in
                package.relationships(&drawing.target)?.into_iter().filter(|r| r.kind == "image")
            {
                shown.push((image.target, sheet.name.clone()));
            }
        }
    }
    Ok(shown)
}
