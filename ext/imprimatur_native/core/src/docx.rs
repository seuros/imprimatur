//! `WordprocessingML` (`.docx`), per ECMA-376 Part 1: the document model as
//! Word paragraphs, runs and tables, with real Heading 1-3 styles so Word's
//! navigation pane and table of contents see them.

mod picture;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::io::Write;

use crate::Result;
use crate::doc::{Block, Document, Paragraph, Picture, Run, Table};
use crate::opc;
use crate::style::{Align, Color, TextDirection};
use crate::xml::XmlWriter;
use crate::zip::ZipWriter;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const WORD_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";
/// A header row's shading.
const HEADER_FILL: &str = "D9E2F3";

pub fn write<W: Write>(document: &Document, out: W) -> Result<W> {
    let room = document.room()?;
    let footer = document.has_page_numbers();
    let mut parts = vec![("styles", "styles.xml".to_owned())];
    if footer {
        parts.push(("footer", "footer1.xml".to_owned()));
    }
    // Pictures' files follow, their relationships numbered from here.
    let first_media = parts.len() + 1;
    // Barcodes go in as pictures.
    let rendered = document
        .blocks()
        .iter()
        .filter_map(|block| match block {
            Block::Picture(picture) => Some(Ok(Cow::Borrowed(picture))),
            Block::Symbol(symbol) => Some(symbol.picture(room).map(Cow::Owned)),
            _ => None,
        })
        .collect::<Result<Vec<Cow<'_, Picture>>>>()?;
    let pictures: Vec<&Picture> = rendered.iter().map(|picture| &**picture).collect();
    for (number, picture) in (1..).zip(&pictures) {
        parts.push(("image", picture::file_name(picture, number)));
    }
    let mut zip = ZipWriter::new(out);
    zip.add("[Content_Types].xml", content_types(footer, &pictures).as_bytes())?;
    let package = opc::relationships(&[("officeDocument", "word/document.xml")]);
    zip.add("_rels/.rels", package.as_bytes())?;
    zip.add("word/document.xml", body(document, room, first_media, &pictures).as_bytes())?;
    zip.add("word/_rels/document.xml.rels", opc::relationships(&parts).as_bytes())?;
    zip.add("word/styles.xml", styles(document).as_bytes())?;
    if footer {
        zip.add("word/footer1.xml", page_number_footer().as_bytes())?;
    }
    for (number, picture) in (1..).zip(&pictures) {
        zip.add(&format!("word/{}", picture::file_name(picture, number)), &picture.data)?;
    }
    zip.finish()
}

/// One override per part: the document, its styles and the footer; the
/// pictures' types by extension.
fn content_types(footer: bool, pictures: &[&Picture]) -> String {
    let mut parts = vec![("/word/document.xml", "document.main"), ("/word/styles.xml", "styles")];
    if footer {
        parts.push(("/word/footer1.xml", "footer"));
    }
    let overrides: Vec<(&str, String)> =
        parts.into_iter().map(|(part, kind)| (part, format!("{WORD_TYPE}.{kind}+xml"))).collect();
    let mut defaults = Vec::new();
    for (extension, content_type) in [("png", "image/png"), ("jpeg", "image/jpeg")] {
        if pictures.iter().any(|picture| picture::extension(picture) == extension) {
            defaults.push((extension, content_type));
        }
    }
    opc::content_types(&defaults, &overrides)
}

/// `word/footer1.xml`: a centered `PAGE / NUMPAGES`, which Word computes.
fn page_number_footer() -> String {
    let mut xml = XmlWriter::new();
    xml.open("w:ftr", &[("xmlns:w", W_NS)]);
    xml.open("w:p", &[]);
    xml.open("w:pPr", &[]);
    xml.empty("w:jc", &[("w:val", "center")]);
    xml.close("w:pPr");
    let field = |xml: &mut XmlWriter, instruction: &str| {
        for kind in ["begin", "instruction", "separate", "placeholder", "end"] {
            xml.open("w:r", &[]);
            match kind {
                "instruction" => {
                    xml.element("w:instrText", &[("xml:space", "preserve")], instruction);
                }
                "placeholder" => xml.element("w:t", &[], "1"),
                _ => xml.empty("w:fldChar", &[("w:fldCharType", kind)]),
            }
            xml.close("w:r");
        }
    };
    field(&mut xml, " PAGE ");
    xml.open("w:r", &[]);
    xml.element("w:t", &[("xml:space", "preserve")], " / ");
    xml.close("w:r");
    field(&mut xml, " NUMPAGES ");
    xml.close("w:p");
    xml.close("w:ftr");
    xml.finish()
}

fn body(
    document: &Document,
    room: (f64, f64),
    first_media: usize,
    pictures: &[&Picture],
) -> String {
    // What reads in no direction of its own (figures, punctuation) takes the
    // document's.
    let rtl = document.reads_right_to_left();
    let mut xml = XmlWriter::new();
    let namespaces = [
        ("xmlns:w", W_NS),
        ("xmlns:r", opc::RELATIONSHIPS),
        ("xmlns:wp", picture::WP_NS),
        ("xmlns:a", picture::A_NS),
        ("xmlns:pic", picture::PIC_NS),
    ];
    xml.open("w:document", &namespaces);
    xml.open("w:body", &[]);
    let mut drawn = 0;
    for block in document.blocks() {
        match block {
            Block::Heading { level, text } => {
                let style = format!("Heading{level}");
                simple_paragraph(&mut xml, Some(&style), text, Look::default(), rtl);
            }
            Block::Paragraph(content) => paragraph(&mut xml, content, rtl),
            Block::Table(content) => table(&mut xml, content, document, rtl),
            Block::Picture(_) | Block::Symbol(_) => {
                drawn += 1;
                let relationship = format!("rId{}", first_media + drawn - 1);
                picture::paragraph(&mut xml, pictures[drawn - 1], room, drawn, &relationship);
            }
            Block::PageBreak => {
                xml.open("w:p", &[]);
                xml.open("w:r", &[]);
                xml.empty("w:br", &[("w:type", "page")]);
                xml.close("w:r");
                xml.close("w:p");
            }
        }
    }
    // Word ends a body that ends with a table with an empty paragraph anyway.
    if matches!(document.blocks().last(), Some(Block::Table(_))) {
        xml.empty("w:p", &[]);
    }
    section(&mut xml, document, rtl);
    xml.close("w:body");
    xml.close("w:document");
    xml.finish()
}

/// Each run reads in the direction of its own first letter, as Word sets
/// them: the Latin runs of a Hebrew paragraph left to right, the Arabic
/// ones of an English paragraph right to left.
fn paragraph(xml: &mut XmlWriter, content: &Paragraph, document_rtl: bool) {
    let rtl = right_to_left(content.effective_direction(), document_rtl);
    open_paragraph(xml, None, rtl, content.align);
    for item in &content.runs {
        run(xml, &item.text, Look::of(item), right_to_left(TextDirection::of(&item.text), rtl));
    }
    xml.close("w:p");
}

/// Whether `direction` is right to left; `otherwise` when there is none:
/// text with no letter (figures, punctuation) takes the direction of what
/// holds it.
fn right_to_left(direction: Option<TextDirection>, otherwise: bool) -> bool {
    direction.map_or(otherwise, |direction| direction == TextDirection::RightToLeft)
}

/// A one-run paragraph straight from borrowed text: headings and table
/// cells, in the direction of their first letter, or of what holds them.
fn simple_paragraph(xml: &mut XmlWriter, style: Option<&str>, text: &str, look: Look, rtl: bool) {
    let rtl = right_to_left(TextDirection::of(text), rtl);
    open_paragraph(xml, style, rtl, None);
    run(xml, text, look, rtl);
    xml.close("w:p");
}

/// `<w:p>` and its properties; the caller writes the runs and closes it.
fn open_paragraph(xml: &mut XmlWriter, style: Option<&str>, rtl: bool, align: Option<Align>) {
    xml.open("w:p", &[]);
    if style.is_none() && !rtl && align.is_none() {
        return;
    }
    xml.open("w:pPr", &[]);
    if let Some(style) = style {
        xml.empty("w:pStyle", &[("w:val", style)]);
    }
    if rtl {
        xml.empty("w:bidi", &[]);
    }
    if let Some(align) = align {
        let value = match align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        xml.empty("w:jc", &[("w:val", value)]);
    }
    xml.close("w:pPr");
}

/// How a run looks, apart from its text.
#[derive(Clone, Copy, Default)]
struct Look {
    bold: bool,
    italic: bool,
    underline: bool,
    color: Option<Color>,
}

impl Look {
    const fn of(run: &Run) -> Self {
        Self { bold: run.bold, italic: run.italic, underline: run.underline, color: run.color }
    }
}

/// One `<w:r>` per piece of text, with `<w:br/>` for `\n` and `<w:tab/>` for `\t`.
fn run(xml: &mut XmlWriter, text: &str, look: Look, rtl: bool) {
    xml.open("w:r", &[]);
    if look.bold || look.italic || look.underline || look.color.is_some() || rtl {
        xml.open("w:rPr", &[]);
        if look.bold {
            xml.empty("w:b", &[]);
        }
        if look.italic {
            xml.empty("w:i", &[]);
        }
        if look.underline {
            xml.empty("w:u", &[("w:val", "single")]);
        }
        if let Some(color) = look.color {
            let hex = color.argb();
            xml.empty("w:color", &[("w:val", &hex[2..])]);
        }
        if rtl {
            xml.empty("w:rtl", &[]);
        }
        xml.close("w:rPr");
    }
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            xml.empty("w:br", &[]);
        }
        for (index, piece) in line.split('\t').enumerate() {
            if index > 0 {
                xml.empty("w:tab", &[]);
            }
            if !piece.is_empty() {
                xml.element("w:t", &[("xml:space", "preserve")], piece);
            }
        }
    }
    xml.close("w:r");
}

fn table(xml: &mut XmlWriter, content: &Table, document: &Document, document_rtl: bool) {
    let rtl = right_to_left(content.direction(), document_rtl);
    let columns = content.columns();
    let (page_width, _) = document.page_size().twips();
    let usable = page_width - 2 * margin(document);
    // Never 0: a table has at least one column.
    let column_width = (usable / u32::try_from(columns).unwrap_or(1).max(1)).to_string();

    xml.open("w:tbl", &[]);
    xml.open("w:tblPr", &[]);
    xml.empty("w:tblStyle", &[("w:val", "TableGrid")]);
    if rtl {
        xml.empty("w:bidiVisual", &[]);
    }
    xml.empty("w:tblW", &[("w:w", "5000"), ("w:type", "pct")]);
    xml.close("w:tblPr");
    xml.open("w:tblGrid", &[]);
    for _ in 0..columns {
        xml.empty("w:gridCol", &[("w:w", &column_width)]);
    }
    xml.close("w:tblGrid");
    for (index, row) in content.rows().iter().enumerate() {
        let header = index == 0 && content.has_header();
        xml.open("w:tr", &[]);
        if header {
            xml.open("w:trPr", &[]);
            xml.empty("w:tblHeader", &[]);
            xml.close("w:trPr");
        }
        for column in 0..columns {
            xml.open("w:tc", &[]);
            xml.open("w:tcPr", &[]);
            xml.empty("w:tcW", &[("w:w", &column_width), ("w:type", "dxa")]);
            if header {
                xml.empty(
                    "w:shd",
                    &[("w:val", "clear"), ("w:color", "auto"), ("w:fill", HEADER_FILL)],
                );
            }
            xml.close("w:tcPr");
            let text = row.get(column).map_or("", String::as_str);
            simple_paragraph(xml, None, text, Look { bold: header, ..Look::default() }, rtl);
            xml.close("w:tc");
        }
        xml.close("w:tr");
    }
    xml.close("w:tbl");
}

/// The document's margin in twips.
fn margin(document: &Document) -> u32 {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 to 2880")]
    let twips = (document.margin() * 20.0).round() as u32;
    twips
}

fn section(xml: &mut XmlWriter, document: &Document, rtl: bool) {
    let (width, height) = document.page_size().twips();
    let (width, height, margin) =
        (width.to_string(), height.to_string(), margin(document).to_string());
    xml.open("w:sectPr", &[]);
    if document.has_page_numbers() {
        // rId2: the footer, after the styles (see `write`).
        xml.empty("w:footerReference", &[("w:type", "default"), ("r:id", "rId2")]);
    }
    xml.empty("w:pgSz", &[("w:w", &width), ("w:h", &height)]);
    let margins =
        [("w:top", &margin), ("w:right", &margin), ("w:bottom", &margin), ("w:left", &margin)];
    xml.empty("w:pgMar", &margins.map(|(key, value)| (key, value.as_str())));
    if rtl {
        xml.empty("w:bidi", &[]);
    }
    xml.close("w:sectPr");
}

/// Heading 1-3: space before and after (twips), color, size (half-points).
const HEADINGS: [(&str, &str, Option<&str>, &str); 3] = [
    ("360", "120", Some("1F3864"), "40"),
    ("240", "80", Some("1F3864"), "30"),
    ("200", "60", None, "26"),
];

/// `word/styles.xml`: Normal text, Heading 1-3 and a bordered table style,
/// with a Calibri body at the document's font size.
fn styles(document: &Document) -> String {
    let mut xml = XmlWriter::new();
    xml.open("w:styles", &[("xmlns:w", W_NS)]);
    xml.open("w:docDefaults", &[]);
    xml.open("w:rPrDefault", &[]);
    xml.open("w:rPr", &[]);
    xml.empty("w:rFonts", &[("w:ascii", "Calibri"), ("w:hAnsi", "Calibri"), ("w:cs", "Arial")]);
    let size = (document.font_size() * 2.0).to_string();
    xml.empty("w:sz", &[("w:val", &size)]);
    xml.empty("w:szCs", &[("w:val", &size)]);
    xml.close("w:rPr");
    xml.close("w:rPrDefault");
    xml.open("w:pPrDefault", &[]);
    xml.open("w:pPr", &[]);
    xml.empty("w:spacing", &[("w:after", "160"), ("w:line", "259"), ("w:lineRule", "auto")]);
    xml.close("w:pPr");
    xml.close("w:pPrDefault");
    xml.close("w:docDefaults");

    xml.open("w:style", &[("w:type", "paragraph"), ("w:default", "1"), ("w:styleId", "Normal")]);
    xml.empty("w:name", &[("w:val", "Normal")]);
    xml.empty("w:qFormat", &[]);
    xml.close("w:style");
    for (level, (before, after, color, size)) in (1..).zip(HEADINGS) {
        let (id, name, outline) =
            (format!("Heading{level}"), format!("heading {level}"), level - 1);
        xml.open("w:style", &[("w:type", "paragraph"), ("w:styleId", &id)]);
        xml.empty("w:name", &[("w:val", &name)]);
        xml.empty("w:basedOn", &[("w:val", "Normal")]);
        xml.empty("w:next", &[("w:val", "Normal")]);
        xml.empty("w:qFormat", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:keepNext", &[]);
        xml.empty("w:spacing", &[("w:before", before), ("w:after", after)]);
        xml.empty("w:outlineLvl", &[("w:val", &outline.to_string())]);
        xml.close("w:pPr");
        xml.open("w:rPr", &[]);
        xml.empty("w:b", &[]);
        if let Some(color) = color {
            xml.empty("w:color", &[("w:val", color)]);
        }
        xml.empty("w:sz", &[("w:val", size)]);
        xml.empty("w:szCs", &[("w:val", size)]);
        xml.close("w:rPr");
        xml.close("w:style");
    }

    xml.open("w:style", &[("w:type", "table"), ("w:styleId", "TableGrid")]);
    xml.empty("w:name", &[("w:val", "Table Grid")]);
    xml.open("w:tblPr", &[]);
    xml.open("w:tblBorders", &[]);
    for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        let line = [("w:val", "single"), ("w:sz", "4"), ("w:space", "0"), ("w:color", "auto")];
        xml.empty(&format!("w:{side}"), &line);
    }
    xml.close("w:tblBorders");
    xml.open("w:tblCellMar", &[]);
    for side in ["w:left", "w:right"] {
        xml.empty(side, &[("w:w", "108"), ("w:type", "dxa")]);
    }
    xml.close("w:tblCellMar");
    xml.close("w:tblPr");
    xml.close("w:style");
    xml.close("w:styles");
    xml.finish()
}
