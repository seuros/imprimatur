use std::assert_matches;

use super::*;
use crate::doc::Document;
use crate::test_support::zip_of;
use crate::xlsx::Workbook;

#[test]
fn tells_formats_apart() {
    let mut book = Workbook::new();
    book.add_sheet("Sales").unwrap();
    let mut doc = Document::new();
    doc.paragraph("Hello.").unwrap();
    let cases = [
        (book.to_bytes().unwrap(), Format::Xlsx, "xlsx"),
        (doc.to_docx().unwrap(), Format::Docx, "docx"),
        (doc.to_pdf().unwrap(), Format::Pdf, "pdf"),
    ];
    for (document, format, name) in cases {
        let read = content(&document, &Limits::default()).unwrap();
        assert_eq!((read.format(), format.as_str()), (format, name));
    }

    let presentation = zip_of(&[
        (
            "[Content_Types].xml",
            &include_str!("docx/tests/content_types.xml")
                .replace("wordprocessingml.document", "presentationml.presentation"),
        ),
        ("_rels/.rels", include_str!("docx/tests/package.rels")),
        ("word/document.xml", "<p/>"),
    ]);
    let zip = zip_of(&[("notes.txt", "a zip of anything")]);
    for document in [b"plain text".as_slice(), &zip, &presentation] {
        let unsupported = content(document, &Limits::default());
        assert_matches!(unsupported, Err(Error::UnsupportedDocument(_)));
        let unsupported = images(document, &Limits::default(), |_| Ok(()));
        assert_matches!(unsupported, Err(Error::UnsupportedDocument(_)));
    }
}
