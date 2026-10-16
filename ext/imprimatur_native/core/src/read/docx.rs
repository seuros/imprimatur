//! DOCX: the body as blocks. Headings from paragraph styles (and outline
//! levels), lists from numbering, bold, italic and links from runs, tables,
//! text boxes after their paragraph. Deleted text, field codes and the
//! fallback copy of alternate content are left out.

mod styles;
#[cfg(test)]
mod tests;

use super::Budget;
use super::content::{Block, Grid, Mark, Text};
use super::markdown::cell_text;
use super::package::Package;
use crate::Result;
use crate::xml::reader::{Event, XmlReader};
use styles::{Style, outline, read_numbering, read_styles, value};

/// Run formatting, and the link a run belongs to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Look {
    bold: bool,
    italic: bool,
    link: Option<usize>,
}

impl Look {
    const fn is_plain(self) -> bool {
        !self.bold && !self.italic && self.link.is_none()
    }
}

/// A paragraph's text, with the look of each stretch of it.
#[derive(Default)]
struct Inline {
    text: String,
    looks: Vec<(std::ops::Range<usize>, Look)>,
}

impl Inline {
    fn push(&mut self, text: &str, look: Look) {
        let start = self.text.len();
        self.text.push_str(text);
        match self.looks.last_mut() {
            Some((range, last)) if *last == look && range.end == start => {
                range.end = self.text.len();
            }
            _ => self.looks.push((start..self.text.len(), look)),
        }
    }

    fn clear(&mut self) {
        self.text.clear();
        self.looks.clear();
    }

    /// The text and its styled stretches, taken: the paragraph is done. A
    /// run's spaces at the edges of a stretch are left out of it.
    fn take(&mut self, links: &[String], styled: bool) -> Text {
        let marks = if styled {
            self.looks
                .iter()
                .filter(|(_, look)| !look.is_plain())
                .filter_map(|(range, look)| {
                    let stretch = &self.text[range.clone()];
                    let start = range.start + (stretch.len() - stretch.trim_start().len());
                    let end = range.end - (stretch.len() - stretch.trim_end().len());
                    (start < end).then(|| Mark {
                        range: start..end,
                        bold: look.bold,
                        italic: look.italic,
                        link: look.link.and_then(|index| links.get(index)).cloned(),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        self.looks.clear();
        Text { text: std::mem::take(&mut self.text), marks }
    }
}

/// What the document's parts say about its paragraphs.
struct Context<'a> {
    budget: &'a Budget<'a>,
    styles: Vec<Style>,
    /// Ordered or not, by numbering id and level.
    numbering: Vec<(String, Vec<bool>)>,
    links: Vec<String>,
    link_ids: Vec<String>,
}

impl Context<'_> {
    /// A style's heading level and list, following `basedOn` a few steps.
    fn style(&self, id: &str) -> (Option<u8>, Option<(String, u8)>) {
        let (mut heading, mut list, mut next) = (None, None, Some(id));
        for _ in 0..8 {
            let Some(style) = next.and_then(|id| self.styles.iter().find(|style| style.id == id))
            else {
                break;
            };
            heading = heading.or(style.heading);
            list = list.or_else(|| style.list.clone());
            next = style.based_on.as_deref();
        }
        (heading, list)
    }

    fn ordered(&self, id: &str, level: u8) -> bool {
        self.numbering
            .iter()
            .find(|(numbering, _)| numbering == id)
            .and_then(|(_, levels)| levels.get(usize::from(level)).copied())
            .unwrap_or(false)
    }
}

/// The paragraph being read.
#[derive(Default)]
struct Paragraph {
    style: Option<String>,
    list: Option<(String, u8)>,
    outline: Option<u8>,
    inline: Inline,
    /// Text boxes anchored in it, which follow it.
    boxes: Vec<Block>,
}

pub fn content(package: &Package<'_>, budget: &Budget<'_>) -> Result<Vec<Block>> {
    let main = package.main();
    let text = package.main_text()?;
    let relationships = package.relationships(main)?;
    let (mut links, mut link_ids) = (Vec::new(), Vec::new());
    for relationship in relationships.iter().filter(|r| r.kind == "hyperlink" && r.external) {
        link_ids.push(relationship.id.clone());
        links.push(relationship.target.clone());
    }
    let styles =
        package.parse_related(&relationships, "styles", |text| read_styles(text, budget))?;
    let numbering =
        package.parse_related(&relationships, "numbering", |text| read_numbering(text, budget))?;
    let context = Context { budget, styles, numbering, links, link_ids };
    let mut xml = XmlReader::new(&text, budget.max_depth());
    let mut out = Vec::new();
    while let Some(event) = xml.next()? {
        if let Event::Start(tag) = event
            && tag.name() == "body"
        {
            blocks(&mut xml, &context, &mut out, "body")?;
            break;
        }
    }
    Ok(out)
}

/// Block content up to the end of `until`: paragraphs, tables, and the
/// containers around them. Returns the grid columns a table cell spans.
fn blocks(
    xml: &mut XmlReader<'_>,
    context: &Context<'_>,
    out: &mut Vec<Block>,
    until: &str,
) -> Result<u32> {
    let mut span = 1;
    let mut paragraph = Paragraph::default();
    while let Some(event) = xml.next()? {
        context.budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "p" => {
                    paragraph.inline.clear();
                    (paragraph.style, paragraph.list, paragraph.outline) = (None, None, None);
                    read_paragraph(xml, context, &mut paragraph)?;
                    push_paragraph(out, &mut paragraph, context);
                }
                "tbl" => {
                    let grid = read_table(xml, context)?;
                    if !grid.is_empty() {
                        out.push(Block::Table(grid));
                    }
                }
                "gridSpan" => {
                    span = tag.attr("val").and_then(|span| span.parse().ok()).unwrap_or(1).max(1);
                }
                "sdtPr" | "sectPr" | "Fallback" | "del" | "moveFrom" => xml.skip()?,
                _ => {}
            },
            Event::End(name) if name == until => return Ok(span),
            _ => {}
        }
    }
    Ok(span)
}

fn read_paragraph(
    xml: &mut XmlReader<'_>,
    context: &Context<'_>,
    paragraph: &mut Paragraph,
) -> Result<()> {
    let mut link = None;
    while let Some(event) = xml.next()? {
        context.budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "pPr" => read_properties(xml, paragraph)?,
                "r" => read_run(xml, context, paragraph, link)?,
                "hyperlink" => {
                    link = tag
                        .attr("id")
                        .and_then(|id| context.link_ids.iter().position(|known| *known == id));
                }
                "del" | "moveFrom" | "Fallback" | "rPr" | "sdtPr" => xml.skip()?,
                _ => {}
            },
            Event::End("hyperlink") => link = None,
            Event::End("p") => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

fn read_properties(xml: &mut XmlReader<'_>, paragraph: &mut Paragraph) -> Result<()> {
    let (mut level, mut numbering) = (None, None);
    while let Some(event) = xml.next()? {
        match event {
            Event::Start(tag) => match tag.name() {
                "pStyle" => paragraph.style = value(&tag),
                "ilvl" => level = styles::level(&tag),
                "numId" => numbering = value(&tag),
                "outlineLvl" => paragraph.outline = outline(&tag),
                "rPr" | "pPrChange" | "sectPr" => xml.skip()?,
                _ => {}
            },
            Event::End("pPr") => break,
            _ => {}
        }
    }
    paragraph.list = numbering.map(|id| (id, level.unwrap_or(0)));
    Ok(())
}

fn read_run(
    xml: &mut XmlReader<'_>,
    context: &Context<'_>,
    paragraph: &mut Paragraph,
    link: Option<usize>,
) -> Result<()> {
    let mut look = Look { link, ..Look::default() };
    let mut in_text = false;
    while let Some(event) = xml.next()? {
        context.budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "rPr" => read_run_properties(xml, &mut look)?,
                "t" => in_text = true,
                "tab" | "ptab" => paragraph.inline.push(" ", look),
                "br" | "cr" => paragraph.inline.push("\n", look),
                "noBreakHyphen" => paragraph.inline.push("-", look),
                "txbxContent" => {
                    blocks(xml, context, &mut paragraph.boxes, "txbxContent")?;
                }
                "delText" | "instrText" | "delInstrText" | "Fallback" => xml.skip()?,
                _ => {}
            },
            Event::Text(text) if in_text => paragraph.inline.push(&text, look),
            Event::End("t") => in_text = false,
            Event::End("r") => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

fn read_run_properties(xml: &mut XmlReader<'_>, look: &mut Look) -> Result<()> {
    while let Some(event) = xml.next()? {
        match event {
            Event::Start(tag) => match tag.name() {
                "b" => look.bold = tag.flag("val"),
                "i" => look.italic = tag.flag("val"),
                "rPrChange" => xml.skip()?,
                _ => {}
            },
            Event::End("rPr") => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

fn read_table(xml: &mut XmlReader<'_>, context: &Context<'_>) -> Result<Grid> {
    let mut grid = Grid::default();
    let (mut row, mut column) = (0_u32, 0_u32);
    let mut cell = Vec::new();
    while let Some(event) = xml.next()? {
        context.budget.tick()?;
        match event {
            Event::Start(tag) => match tag.name() {
                "tr" => column = 0,
                "tc" => {
                    cell.clear();
                    let span = blocks(xml, context, &mut cell, "tc")?;
                    cell_text(&cell, grid.buffer());
                    grid.end_cell(row, column, super::content::Value::Text);
                    column += span;
                }
                "tblPr" | "tblGrid" | "trPr" => xml.skip()?,
                _ => {}
            },
            Event::End("tr") => row += 1,
            Event::End("tbl") => return Ok(grid.finish()),
            _ => {}
        }
    }
    Ok(grid.finish())
}

/// The paragraph as a block, unless it has no text; then the blocks of its
/// text boxes.
fn push_paragraph(out: &mut Vec<Block>, paragraph: &mut Paragraph, context: &Context<'_>) {
    let (styled_heading, styled_list) =
        paragraph.style.as_deref().map_or((None, None), |style| context.style(style));
    let heading = paragraph.outline.or(styled_heading);
    let list = paragraph.list.clone().or(styled_list).filter(|(id, _)| id != "0");
    if !paragraph.inline.text.trim().is_empty() {
        out.push(if let Some(level) = heading {
            Block::Heading { level, text: paragraph.inline.take(&context.links, false) }
        } else if let Some((id, level)) = list {
            let ordered = context.ordered(&id, level);
            Block::ListItem { level, ordered, text: paragraph.inline.take(&context.links, true) }
        } else {
            Block::Paragraph { text: paragraph.inline.take(&context.links, true) }
        });
    }
    out.append(&mut paragraph.boxes);
}
