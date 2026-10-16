//! Content as Markdown. Escaping is minimal on purpose: the text feeds
//! search indexes and language models, where `snake\_case` would be noise.
//! Only what would break the structure is escaped: pipes and line breaks
//! inside table cells, and a paragraph that would read as a heading or a
//! quote.

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

use super::content::{Block, Cell, Content, Grid, Page, Sheet, Text, push_reference};

/// The deepest Markdown heading.
const MAX_HEADING: u8 = 6;

impl Content {
    /// The content as Markdown: a PDF's pages (from the second, marked
    /// `<!-- page N -->`), each sheet of a workbook under its name, a Word
    /// document's blocks.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Pdf(pages) => pdf(&mut out, pages),
            Self::Xlsx(sheets) => {
                for sheet in sheets {
                    write_sheet(&mut out, sheet);
                }
            }
            Self::Docx(blocks) => write_blocks(&mut out, blocks.iter(), Mode::Body),
        }
        out
    }
}

/// How a block's text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Emphasis and links.
    Body,
    /// Links only: emphasis in a table cell is mostly a bold header row.
    Cell,
}

fn pdf(out: &mut String, pages: &[Page]) {
    for page in pages.iter().filter(|page| !page.blocks.is_empty()) {
        if page.number > 1 {
            end_block(out);
            // Writing to a String cannot fail.
            let _ = writeln!(out, "<!-- page {} -->", page.number);
        }
        write_blocks(out, page.blocks.iter().map(|placed| &placed.block), Mode::Body);
    }
}

fn write_sheet(out: &mut String, sheet: &Sheet) {
    end_block(out);
    out.push_str("## ");
    out.push_str(&sheet.name);
    if sheet.hidden {
        out.push_str(" (hidden)");
    }
    out.push_str("\n\n");
    if sheet.cells.is_empty() {
        out.push_str("_Empty._\n");
    } else {
        write_table(out, &sheet.cells);
    }
    for (index, note) in sheet.comments.iter().enumerate() {
        if index == 0 {
            end_block(out);
            out.push_str("Comments:\n\n");
        }
        out.push_str("- ");
        push_reference(out, note.row, note.column);
        if let Some(author) = &note.author {
            out.push_str(" (");
            out.push_str(author);
            out.push(')');
        }
        out.push_str(": ");
        for (index, line) in note.text.lines().enumerate() {
            if index > 0 {
                out.push_str("\n  ");
            }
            out.push_str(line.trim_end());
        }
        out.push('\n');
    }
}

/// Blocks in order: consecutive list items form one list; everything else
/// is a block of its own.
pub fn write_blocks<'b>(out: &mut String, blocks: impl Iterator<Item = &'b Block>, mode: Mode) {
    let mut in_list = false;
    for block in blocks {
        match block {
            Block::Heading { level, text } => {
                end_block(out);
                for _ in 0..(*level).clamp(1, MAX_HEADING) {
                    out.push('#');
                }
                out.push(' ');
                out.extend(text.text.chars().map(|ch| if ch == '\n' { ' ' } else { ch }));
                out.push('\n');
            }
            Block::Paragraph { text } => {
                end_block(out);
                if text.text.starts_with(['#', '>']) {
                    out.push('\\');
                }
                write_text(out, text, mode, "");
                out.push('\n');
            }
            Block::ListItem { level, ordered, text } => {
                if !in_list {
                    end_block(out);
                }
                let indent = "  ".repeat(usize::from(*level));
                let marker = if *ordered { "1. " } else { "- " };
                out.push_str(&indent);
                out.push_str(marker);
                let continuation = format!("{indent}{}", " ".repeat(marker.len()));
                write_text(out, text, mode, &continuation);
                out.push('\n');
            }
            Block::Table(grid) => {
                end_block(out);
                write_table(out, grid);
            }
        }
        in_list = matches!(block, Block::ListItem { .. });
    }
}

/// A cell's blocks as one cell of a table: links kept, emphasis left out,
/// each line on its own, blank lines dropped.
pub fn cell_text(blocks: &[Block], out: &mut String) {
    let mut rendered = String::new();
    write_blocks(&mut rendered, blocks.iter(), Mode::Cell);
    for line in rendered.lines().filter(|line| !line.trim().is_empty()) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
}

/// Text with its marks; line breaks continue at `indent`, as a list item's
/// must. Whitespace at the edges of a marked stretch stays outside it.
fn write_text(out: &mut String, text: &Text, mode: Mode, indent: &str) {
    let mut at = 0;
    for mark in &text.marks {
        push_lines(out, &text.text[at..mark.range.start], indent);
        let stretch = &text.text[mark.range.clone()];
        at = mark.range.end;
        let core = stretch.trim();
        if core.is_empty() {
            push_lines(out, stretch, indent);
            continue;
        }
        push_lines(out, &stretch[..stretch.len() - stretch.trim_start().len()], indent);
        let emphasis = mode == Mode::Body;
        let marker = match (mark.bold && emphasis, mark.italic && emphasis) {
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            (false, false) => "",
        };
        if mark.link.is_some() {
            out.push('[');
        }
        out.push_str(marker);
        push_lines(out, core, indent);
        out.push_str(marker);
        if let Some(url) = &mark.link {
            out.push_str("](");
            out.push_str(url);
            out.push(')');
        }
        push_lines(out, &stretch[stretch.trim_end().len()..], indent);
    }
    push_lines(out, &text.text[at..], indent);
}

fn push_lines(out: &mut String, text: &str, indent: &str) {
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
            out.push_str(indent);
        }
        out.push_str(line);
    }
}

/// A grid as a table: its first row as the header, rows without cells and
/// columns without any left out.
pub fn write_table(out: &mut String, grid: &Grid) {
    let Some(last) = grid.cells().iter().map(|cell| cell.column).max() else { return };
    let mut used = vec![false; last as usize + 1];
    for cell in grid.cells() {
        used[cell.column as usize] = true;
    }
    let columns: Vec<u32> = (0..=last).filter(|&column| used[column as usize]).collect();
    let mut rows = grid.rows();
    let Some(header) = rows.next() else { return };
    write_row(out, grid, header, &columns);
    out.push('|');
    for _ in &columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows {
        write_row(out, grid, row, &columns);
    }
}

fn write_row(out: &mut String, grid: &Grid, cells: &[Cell], columns: &[u32]) {
    out.push('|');
    let mut cells = cells.iter().peekable();
    for &column in columns {
        out.push(' ');
        if let Some(cell) = cells.next_if(|cell| cell.column == column) {
            push_cell(out, grid.text(cell));
        }
        out.push_str(" |");
    }
    out.push('\n');
}

/// Cell text on one line: pipes escaped, line breaks as `<br>`.
pub fn push_cell(out: &mut String, text: &str) {
    for (index, line) in text.trim().lines().enumerate() {
        if index > 0 {
            out.push_str("<br>");
        }
        for ch in line.trim_end_matches('\r').chars() {
            if ch == '|' {
                out.push('\\');
            }
            out.push(ch);
        }
    }
}

/// Ends the current block with a blank line, unless the output is empty
/// or already ends with one.
pub fn end_block(out: &mut String) {
    if out.is_empty() || out.ends_with("\n\n") {
        return;
    }
    out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
}
