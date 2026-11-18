use super::*;
use crate::read::content::{Mark, Note, Placed, Value};

#[test]
fn keeps_an_empty_pages_marker_so_the_page_count_still_matches() {
    // `--drop-repeated` (or a genuinely blank page) can leave a page with no
    // blocks; its `<!-- page N -->` marker must still show up, or a caller
    // mapping markers back to PDF page numbers loses the count.
    let page = |number, text: &str| Page {
        number,
        blocks: if text.is_empty() {
            vec![]
        } else {
            vec![Placed { block: Block::Paragraph { text: Text::plain(text) }, bounds: None }]
        },
        images: 0,
        label: None,
    };
    let pages = [page(1, "Body 1"), page(2, ""), page(3, "Body 3")];
    assert_eq!(
        Content::Pdf(pages.to_vec()).to_markdown(),
        "Body 1\n\n<!-- page 2 -->\n\n<!-- page 3 -->\n\nBody 3\n"
    );
}

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
        ..Sheet::default()
    };
    let blank = Sheet { name: "Blank".to_owned(), ..Sheet::default() };
    assert_eq!(
        Content::Xlsx(vec![sheet, blank]).to_markdown(),
        "## Sales (hidden)\n\n| Region |\n| --- |\n| 12.5 |\n\n\
         Comments:\n\n- A1 (Finance): first\n  second\n\n## Blank\n\n_Empty._\n"
    );
}

/// The lines of `text` a 1-based inclusive range covers.
fn slice(text: &str, lines: &RangeInclusive<usize>) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(index, _)| lines.contains(&(index + 1)))
        .map(|(_, line)| line.to_owned())
        .collect()
}

#[test]
fn outlines_pages_by_line() {
    let page = |number, text: &str, label: Option<&str>| Page {
        number,
        blocks: if text.is_empty() {
            vec![]
        } else {
            vec![Placed { block: Block::Paragraph { text: Text::plain(text) }, bounds: None }]
        },
        images: 0,
        label: label.map(str::to_owned),
    };
    let content = Content::Pdf(vec![
        page(1, "Body 1", None),
        page(2, "", None),
        page(3, "Body 3", Some("iii")),
    ]);
    let (text, sections) = content.to_markdown_outline();
    assert_eq!(text, content.to_markdown());
    let pages: Vec<(u32, Vec<String>)> = sections
        .iter()
        .map(|section| {
            let SectionKind::Page { number, .. } = section.kind else {
                panic!("a PDF outlines pages, got {section:?}");
            };
            (number, slice(&text, &section.lines))
        })
        .collect();
    assert_eq!(
        pages,
        [
            (1, vec!["Body 1".to_owned()]),
            (2, vec!["<!-- page 2 -->".to_owned()]),
            (3, vec!["<!-- page 3 (label iii) -->".to_owned(), String::new(), "Body 3".to_owned()]),
        ]
    );
    assert_eq!(sections[2].kind, SectionKind::Page { number: 3, label: Some("iii".to_owned()) });
}

#[test]
fn outlines_a_blank_first_page_as_empty() {
    let blank = Page { number: 1, blocks: vec![], images: 0, label: None };
    let (text, sections) = Content::Pdf(vec![blank]).to_markdown_outline();
    assert_eq!(text, "");
    assert!(sections[0].lines.is_empty());
}

#[test]
fn outlines_sheets_with_their_table_header_and_rows() {
    let mut cells = Grid::default();
    cells.push(0, 0, "Material");
    cells.push(0, 1, "Share");
    cells.push(1, 0, "Cotton");
    cells.push(1, 1, "95%");
    cells.push(2, 0, "Elastane");
    cells.push(2, 1, "5%");
    let bom = Sheet {
        name: "BOM".to_owned(),
        cells: cells.finish(),
        comments: vec![Note { row: 1, column: 0, author: None, text: "organic".to_owned() }],
        ..Sheet::default()
    };
    let mut header_only = Grid::default();
    header_only.push(0, 0, "Supplier");
    let suppliers =
        Sheet { name: "Suppliers".to_owned(), cells: header_only.finish(), ..Sheet::default() };
    let blank = Sheet { name: "Blank".to_owned(), ..Sheet::default() };
    let content = Content::Xlsx(vec![bom, suppliers, blank]);

    let (text, sections) = content.to_markdown_outline();
    assert_eq!(text, content.to_markdown());

    let SectionKind::Sheet { name, table: Some(table), .. } = &sections[0].kind else {
        panic!("{:?}", sections[0]);
    };
    assert_eq!(name, "BOM");
    assert_eq!(
        slice(&text, &sections[0].lines),
        [
            "## BOM",
            "",
            "| Material | Share |",
            "| --- | --- |",
            "| Cotton | 95% |",
            "| Elastane | 5% |",
            "",
            "Comments:",
            "",
            "- A2: organic"
        ]
    );
    assert_eq!(slice(&text, &table.header), ["| Material | Share |", "| --- | --- |"]);
    assert_eq!(slice(&text, &table.rows), ["| Cotton | 95% |", "| Elastane | 5% |"]);
    assert_eq!(table.row_count(), 2);

    let SectionKind::Sheet { table: Some(table), .. } = &sections[1].kind else {
        panic!("{:?}", sections[1]);
    };
    assert_eq!(slice(&text, &table.header), ["| Supplier |", "| --- |"]);
    assert_eq!(table.row_count(), 0);

    let SectionKind::Sheet { table: None, .. } = &sections[2].kind else {
        panic!("an empty sheet has no table: {:?}", sections[2]);
    };
    assert_eq!(slice(&text, &sections[2].lines), ["## Blank", "", "_Empty._"]);
}
