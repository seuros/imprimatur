use std::assert_matches;

use crate::Error;
use crate::barcode::{Barcode, Level};
use crate::doc::{Document, PageSize, Paragraph, Picture, Run, Symbol, Table};
use crate::read::{
    ImageFormat, ImageLocation, Limits, PngColor, images, png, png_with_transparency,
};
use crate::style::{Align, Color};

/// Checks the file's structure and returns each page's decompressed content.
fn pages(pdf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf);
    assert!(pdf.starts_with(b"%PDF-1.7\n"));
    assert!(text.ends_with("%%EOF\n"));
    let startxref: usize =
        text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(pdf[startxref..].starts_with(b"xref\n"), "startxref points at the table");
    // Byte offsets: the lossy text is longer than the bytes (binary header).
    let table = String::from_utf8_lossy(&pdf[startxref..]);
    let size: usize = table.lines().nth(1).unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    for (id, entry) in table.lines().skip(3).take(size - 1).enumerate() {
        let offset: usize = entry[..10].parse().unwrap();
        let expected = format!("{} 0 obj\n", id + 1);
        assert!(pdf[offset..].starts_with(expected.as_bytes()), "object {} offset", id + 1);
    }
    let mut contents = Vec::new();
    let mut rest = pdf;
    while let Some(start) = find(rest, b"stream\n") {
        let body = &rest[start + 7..];
        let end = find(body, b"\nendstream").unwrap();
        let object = rest[..start].windows(5).rposition(|at| at == b" obj\n").unwrap();
        if find(&rest[object..start], b"/Subtype /Image").is_none() {
            let data = miniz_oxide::inflate::decompress_to_vec_zlib(&body[..end]).unwrap();
            contents.push(String::from_utf8_lossy(&data).into_owned());
        }
        rest = &body[end + b"\nendstream".len()..];
    }
    let count: usize =
        text.split("/Count ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert_eq!(contents.len(), count, "one content stream per page");
    contents
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[test]
fn writes_a_valid_single_page() {
    let mut doc = Document::new();
    doc.heading(1, "Report (draft)").unwrap();
    doc.push_paragraph(
        Paragraph::new().text("Plain, ").run(Run::new("bold").bold().color(Color::rgb(0xFF_00_00))),
    )
    .unwrap();
    let pdf = doc.to_pdf().unwrap();
    let pages = pages(&pdf);
    assert_eq!(pages.len(), 1);
    assert!(pages[0].contains(r"/F2 20 Tf"), "heading in bold 20pt: {}", pages[0]);
    assert!(pages[0].contains(r"(Report \(draft\)) Tj"), "parentheses escaped: {}", pages[0]);
    assert!(pages[0].contains("1 0 0 rg"), "red run: {}", pages[0]);
    assert!(
        String::from_utf8_lossy(&pdf)
            .contains("/BaseFont /Helvetica-BoldOblique /Encoding /WinAnsiEncoding")
    );
    assert_eq!(doc.to_pdf().unwrap(), pdf, "deterministic");
}

#[test]
fn uses_the_body_font_size_for_every_run_style() {
    let mut doc = Document::new();
    doc.set_page_numbers(true);
    for level in 1..=3 {
        doc.heading(level, "Heading").unwrap();
    }
    doc.push_paragraph(
        Paragraph::new()
            .text("Plain ")
            .run(Run::new("bold ").bold())
            .run(Run::new("italic ").italic())
            .run(Run::new("both").bold().italic().underline()),
    )
    .unwrap();
    doc.table(Table::new([["Header"], ["Cell"]]).header()).unwrap();
    // Page setup also applies to content added before it.
    doc.set_font_size(10.5).unwrap();
    let pages = pages(&doc.to_pdf().unwrap());
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    for font in ["F1", "F2", "F3", "F4"] {
        assert!(page.contains(&format!("/{font} 10.5 Tf")), "{page}");
    }
    for unchanged in ["/F2 20 Tf", "/F2 15 Tf", "/F2 12.5 Tf", "/F2 10 Tf", "/F1 10 Tf", "/F1 9 Tf"]
    {
        assert!(page.contains(unchanged), "{page}");
    }
}

#[test]
fn paginates_and_numbers_pages() {
    let mut doc = Document::new();
    doc.set_page_numbers(true);
    for index in 0..120 {
        doc.paragraph(format!("Paragraph {index}: the quick brown fox jumps over the lazy dog."))
            .unwrap();
    }
    doc.page_break().paragraph("After the break.").unwrap();
    let pages = pages(&doc.to_pdf().unwrap());
    assert!(pages.len() >= 4, "{} pages", pages.len());
    let total = pages.len();
    for (index, page) in pages.iter().enumerate() {
        assert!(page.contains(&format!("({} / {total}) Tj", index + 1)), "page {index}");
    }
    assert!(pages.last().unwrap().contains("(After the break.) Tj"));
    assert!(!pages[total - 2].contains("After the break"));
}

#[test]
fn aligns_lines() {
    let mut doc = Document::new();
    doc.push_paragraph(Paragraph::new().text("x").align(Align::Right)).unwrap();
    let page = &pages(&doc.to_pdf().unwrap())[0];
    // A4 is 595.28pt wide; one-inch margins; "x" is 5.5pt at 11pt.
    assert!(page.contains("517.78 "), "right aligned at 595.28 - 72 - 5.5: {page}");
}

#[test]
fn repeats_table_headers_across_pages() {
    let mut rows = vec![vec!["Region".to_owned(), "Amount".to_owned()]];
    rows.extend((0..80).map(|index| vec![format!("Region {index}"), format!("{index}.00")]));
    let mut doc = Document::new();
    doc.table(Table::new(rows).header()).unwrap();
    let pages = pages(&doc.to_pdf().unwrap());
    assert!(pages.len() >= 2);
    for page in &pages {
        assert!(
            page.contains("/F2 10 Tf 0 0 0 rg") && page.contains("(Region) Tj"),
            "header on every page"
        );
        assert!(page.contains("0.85 0.89 0.95 rg"), "header shading");
    }
}

#[test]
fn refuses_what_it_cannot_draw() {
    let mut doc = Document::new();
    doc.paragraph("مرحبا").unwrap();
    assert_matches!(
        doc.to_pdf(),
        Err(Error::UnsupportedCharacter { character: 'م', font: "Helvetica" })
    );
    assert!(doc.to_docx().is_ok(), "DOCX carries any script");

    let mut tall = Document::new();
    tall.table(Table::new([["line\n".repeat(80)]])).unwrap();
    assert_matches!(tall.to_pdf(), Err(Error::RowTooTall(_)));
}

#[test]
fn an_empty_document_is_one_blank_page() {
    assert_eq!(pages(&Document::new().to_pdf().unwrap()), [""]);
}

#[test]
fn draws_pictures() {
    let gray = png(2, 2, PngColor::Gray, 8, None, &[0, 255, 255, 0]);
    let palette = [255, 0, 0, 0, 0, 255];
    let indexed = png(4, 1, PngColor::Palette, 2, Some(&palette), &[0b0001_0001]);
    // The second color fully transparent.
    let transparent = png_with_transparency(
        4,
        1,
        PngColor::Palette,
        2,
        Some(&palette),
        b"\xFF\x00",
        &[0b0001_0001],
    );
    let rgba = png(1, 1, PngColor::Rgba, 8, None, &[10, 20, 30, 128]);
    // An 8-bit, 3 by 2, RGB baseline frame header.
    let jpeg =
        [0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8, 0, 2, 0, 3, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];

    let mut doc = Document::new();
    doc.set_page_size(PageSize::custom(200.0, 200.0).unwrap()).set_margin(10.0).unwrap();
    let pictures = [gray, indexed, transparent, rgba, jpeg.to_vec()];
    for (data, align) in
        pictures.into_iter().zip([Align::Left, Align::Center, Align::Right].iter().cycle())
    {
        doc.picture(Picture::new(data).unwrap().width(40.0).unwrap().align(*align));
    }
    let pdf = doc.to_pdf().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("/MediaBox [0 0 200 200]"));
    assert_eq!(pages(&pdf).len(), 1, "drawn 40 points wide, one under another");

    let mut found = Vec::new();
    images(&pdf, &Limits::default(), |image| {
        found.push(image);
        Ok(())
    })
    .unwrap();
    let placed: Vec<_> = found
        .iter()
        .map(|image| {
            let ImageLocation::Page { number, bounds: Some(bounds) } = &image.location else {
                panic!("{:?}", image.location)
            };
            (image.format, image.width, image.height, *number, bounds.left, bounds.top)
        })
        .collect();
    assert_eq!(
        placed,
        [
            (ImageFormat::Png, Some(2), Some(2), 1, 10.0, 10.0),
            (ImageFormat::Png, Some(4), Some(1), 1, 80.0, 56.0),
            (ImageFormat::Png, Some(4), Some(1), 1, 150.0, 72.0),
            (ImageFormat::Png, Some(1), Some(1), 1, 10.0, 88.0),
            (ImageFormat::Jpeg, Some(3), Some(2), 1, 80.0, 134.0),
        ]
    );
    assert_eq!(found[3].data[25], 6, "its alpha comes back: RGBA");
    assert_matches!(Picture::new(b"GIF89a".to_vec()), Err(Error::InvalidImage(_)));
}

#[test]
fn draws_barcodes_as_shapes() {
    let mut doc = Document::new();
    doc.set_page_size(PageSize::custom(300.0, 400.0).unwrap()).set_margin(10.0).unwrap();
    // At its usual size: 0.935 points a module, 113 modules wide with the
    // quiet zone, bars 69 modules high.
    let ean = Barcode::ean13("9506000134352").unwrap();
    doc.barcode(ean.clone());
    let qr = Barcode::qr("https://example.com", Level::Medium).unwrap();
    let side = qr.size().0 + 8;
    let rects = ean.shapes(true, None).rects.len() + qr.shapes(true, None).rects.len();
    let symbol = Symbol::new(qr).width(116.0).unwrap().align(Align::Right);
    doc.barcode(symbol.color(Color::rgb(0x1F_3864)));
    let pages = pages(&doc.to_pdf().unwrap());
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    // From the top margin down, flipped: the EAN's top at 390.
    assert!(page.starts_with("q 0 0 0 rg 0.9350 0 0 -0.9350 10 390 cm\n11 0 1 69 re\n"), "{page}");
    let qr_module = 116.0 / f64::from(u32::try_from(side).unwrap());
    let qr_top = 69.0f64.mul_add(-0.935, 390.0) - 6.0;
    let expected =
        format!("q 0.12 0.22 0.39 rg {qr_module:.4} 0 0 {:.4} 174 {qr_top:.2} cm\n", -qr_module);
    assert!(page.contains(&expected), "{expected}\n{page}");
    assert_eq!(page.matches(" re\n").count(), rects, "one rectangle a run");
    assert_eq!(page.matches("f Q\n").count(), 2);
}
