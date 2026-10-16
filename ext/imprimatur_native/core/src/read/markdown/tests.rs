use super::*;
use crate::read::content::{Mark, Note, Value};

#[test]
fn writes_sparse_tables() {
    let mut grid = Grid::default();
    grid.push(0, 0, "Name");
    grid.push(0, 3, "Unit");
    grid.push(2, 0, "a | b");
    grid.push(2, 3, "line\nbreak");
    grid.push(3, 1, "  ");
    grid.push(1, 0, "late");
    grid.push(1, 0, "duplicate");
    let mut out = String::new();
    write_table(&mut out, &grid.finish());
    assert_eq!(
        out, "| Name | Unit |\n| --- | --- |\n| late |  |\n| a \\| b | line<br>break |\n",
        "empty columns and rows dropped, rows sorted, the first of two cells kept"
    );
}

#[test]
fn ends_blocks_with_one_blank_line() {
    for (before, after) in [("", ""), ("a", "a\n\n"), ("a\n", "a\n\n"), ("a\n\n", "a\n\n")] {
        let mut out = before.to_owned();
        end_block(&mut out);
        assert_eq!(out, after, "{before:?}");
    }
}

#[test]
fn writes_marks_lists_and_escapes() {
    let mark = |range, bold, link: Option<&str>| Mark {
        range,
        bold,
        italic: false,
        link: link.map(str::to_owned),
    };
    let text = Text {
        text: "say  hello there\nnow".to_owned(),
        marks: vec![mark(3..10, true, None), mark(11..16, false, Some("https://x.test"))],
    };
    let blocks = [
        Block::Paragraph { text: Text::plain("# not a heading") },
        Block::ListItem { level: 0, ordered: false, text },
        Block::ListItem { level: 1, ordered: true, text: Text::plain("nested") },
        Block::Heading { level: 9, text: Text::plain("deep\nheading") },
    ];
    let mut out = String::new();
    write_blocks(&mut out, blocks.iter(), Mode::Body);
    assert_eq!(
        out,
        "\\# not a heading\n\n- say  **hello** [there](https://x.test)\n  now\n  1. nested\n\n\
         ###### deep heading\n"
    );
    let mut cell = String::new();
    cell_text(&blocks[1..2], &mut cell);
    assert_eq!(cell, "- say  hello [there](https://x.test)\n  now", "links kept, emphasis dropped");
}

#[test]
fn writes_sheets_with_comments() {
    let mut cells = Grid::default();
    cells.push(0, 0, "Region");
    cells.buffer().push_str("12.5");
    cells.end_cell(1, 0, Value::Number(12.5));
    let sheet = Sheet {
        name: "Sales".to_owned(),
        hidden: true,
        cells: cells.finish(),
        comments: vec![Note {
            row: 0,
            column: 0,
            author: Some("Finance".into()),
            text: "first  \nsecond".to_owned(),
        }],
    };
    let blank = Sheet {
        name: "Blank".to_owned(),
        hidden: false,
        cells: Grid::default(),
        comments: Vec::new(),
    };
    assert_eq!(
        Content::Xlsx(vec![sheet, blank]).to_markdown(),
        "## Sales (hidden)\n\n| Region |\n| --- |\n| 12.5 |\n\n\
         Comments:\n\n- A1 (Finance): first\n  second\n\n## Blank\n\n_Empty._\n"
    );
}
