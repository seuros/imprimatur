use serde_json::json;

use super::*;
use crate::read::{Limits, Note, Placed, Text, content as read};
use crate::xlsx::{Cell as Written, Workbook};

#[test]
fn types_cells_and_keeps_shown_text_apart() {
    let mut book = Workbook::new();
    let sheet = book.add_sheet("Ledger").unwrap();
    sheet.append_row([Written::from("Fee"), Written::from(1250.5), Written::from(true)]).unwrap();
    sheet.append_row([Written::formula("SUM(B1:B1)")]).unwrap();
    let mut content = read(&book.to_bytes().unwrap(), &Limits::default()).unwrap();
    let Content::Xlsx(sheets) = &mut content else { panic!() };
    sheets[0].comments.push(Note { row: 0, column: 1, author: None, text: "net".into() });
    let expected = json!({
        "format": "xlsx",
        "sheets": [{
            "name": "Ledger",
            "hidden": false,
            "rows": [
                { "row": 1, "cells": [
                    { "cell": "A1", "type": "text", "value": "Fee" },
                    { "cell": "B1", "type": "number", "value": 1250.5, "text": "1250.5" },
                    { "cell": "C1", "type": "bool", "value": true, "text": "TRUE" },
                ] },
                { "row": 2, "cells": [
                    { "cell": "A2", "type": "formula", "value": "=SUM(B1:B1)" },
                ] },
            ],
            "comments": [{ "cell": "B1", "author": null, "text": "net" }],
        }],
    });
    assert_eq!(Json::Object(super::content(&content)), expected);
}

#[test]
fn places_blocks_and_marks_their_stretches() {
    let link =
        Mark { range: 4..8, bold: false, italic: false, link: Some("https://x.test".into()) };
    let text = Text { text: "see here".into(), marks: vec![link] };
    let bounds = Bounds { left: 72.0, top: 100.004, right: 300.0, bottom: 112.5 };
    let pdf = Content::Pdf(vec![Page {
        number: 1,
        blocks: vec![
            Placed {
                block: Block::Heading { level: 1, text: Text::plain("Scope") },
                bounds: Some(bounds),
            },
            Placed { block: Block::Paragraph { text }, bounds: None },
        ],
        images: 2,
    }]);
    let expected = json!({
        "format": "pdf",
        "pages": [{ "page": 1, "images": 2, "blocks": [
            { "type": "heading", "level": 1, "text": "Scope",
              "bounds": { "left": 72.0, "top": 100.0, "right": 300.0, "bottom": 112.5 } },
            { "type": "paragraph", "text": "see here", "marks": [
                { "start": 4, "end": 8, "bold": false, "italic": false, "link": "https://x.test" },
            ] },
        ] }],
    });
    assert_eq!(Json::Object(super::content(&pdf)), expected);
}

#[test]
fn describes_images_by_where_they_are() {
    let bounds = Some(Bounds { left: 0.0, top: 1.0, right: 2.0, bottom: 3.0 });
    let cases = [
        (
            ImageLocation::Page { number: 3, bounds },
            json!({ "format": "png", "width": 2, "height": 1, "bytes": 4, "page": 3,
                    "bounds": { "left": 0.0, "top": 1.0, "right": 2.0, "bottom": 3.0 } }),
        ),
        (
            ImageLocation::Part { name: "xl/media/image1.png".into(), sheet: Some("Logo".into()) },
            json!({ "format": "png", "width": 2, "height": 1, "bytes": 4,
                    "part": "xl/media/image1.png", "sheet": "Logo" }),
        ),
    ];
    for (location, expected) in cases {
        let image = Image {
            data: vec![0; 4],
            format: crate::read::ImageFormat::Png,
            width: Some(2),
            height: Some(1),
            location,
        };
        assert_eq!(Json::Object(super::image(&image)), expected);
    }
}
