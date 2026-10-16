//! What a DOCX's parts besides the body say about its paragraphs: heading
//! levels and lists from `styles.xml`, list kinds from `numbering.xml`.

use crate::Result;
use crate::read::Budget;
use crate::xml::reader::{Event, Tag, XmlReader};

/// What a paragraph style contributes.
#[derive(Debug, Clone, Default)]
pub struct Style {
    pub id: String,
    pub based_on: Option<String>,
    pub heading: Option<u8>,
    pub list: Option<(String, u8)>,
}

/// A `w:val` attribute, owned.
pub fn value(tag: &Tag<'_>) -> Option<String> {
    tag.attr("val").map(std::borrow::Cow::into_owned)
}

/// A `w:val` list level (`w:ilvl`), 0 to 8.
pub fn level(tag: &Tag<'_>) -> Option<u8> {
    tag.attr("val").and_then(|level| level.parse::<u8>().ok())
}

/// `w:outlineLvl`: 0 is a level-1 heading; 9 is body text.
pub fn outline(tag: &Tag<'_>) -> Option<u8> {
    level(tag).filter(|&level| level < 9).map(|level| level + 1)
}

pub fn read_styles(text: &str, budget: &Budget<'_>) -> Result<Vec<Style>> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let (mut styles, mut style): (Vec<Style>, Option<Style>) = (Vec::new(), None);
    let (mut list_level, mut numbering) = (None, None);
    while let Some(event) = xml.next()? {
        match event {
            Event::Start(tag) => match (tag.name(), style.as_mut()) {
                ("style", _) => {
                    let paragraph = tag.attr("type").is_none_or(|kind| kind == "paragraph");
                    style = paragraph.then(|| Style {
                        id: tag.attr("styleId").unwrap_or_default().into_owned(),
                        ..Style::default()
                    });
                    (list_level, numbering) = (None, None);
                }
                ("name", Some(style)) => {
                    let name = tag.attr("val").unwrap_or_default().to_ascii_lowercase();
                    style.heading = style.heading.or_else(|| match name.as_str() {
                        "title" => Some(1),
                        name => name
                            .strip_prefix("heading ")
                            .and_then(|level| level.trim().parse().ok()),
                    });
                }
                ("basedOn", Some(style)) => {
                    style.based_on = value(&tag);
                }
                ("outlineLvl", Some(style)) => {
                    style.heading = style.heading.or_else(|| outline(&tag));
                }
                ("ilvl", Some(_)) => {
                    list_level = self::level(&tag);
                }
                ("numId", Some(_)) => numbering = value(&tag),
                _ => {}
            },
            Event::End("style") => {
                if let Some(mut done) = style.take() {
                    done.list = numbering.take().map(|id| (id, list_level.unwrap_or(0)));
                    styles.push(done);
                }
            }
            _ => {}
        }
    }
    Ok(styles)
}

/// Which list levels are numbered rather than bulleted, by numbering id.
pub fn read_numbering(text: &str, budget: &Budget<'_>) -> Result<Vec<(String, Vec<bool>)>> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let mut abstracts: Vec<(String, Vec<bool>)> = Vec::new();
    let mut numbers: Vec<(String, String)> = Vec::new();
    let mut current_number: Option<String> = None;
    while let Some(event) = xml.next()? {
        match event {
            Event::Start(tag) => match tag.name() {
                "abstractNum" => abstracts
                    .push((tag.attr("abstractNumId").unwrap_or_default().into_owned(), Vec::new())),
                "numFmt" if current_number.is_none() => {
                    if let Some((_, levels)) = abstracts.last_mut() {
                        let format = tag.attr("val").unwrap_or_default();
                        levels.push(!matches!(&*format, "bullet" | "none"));
                    }
                }
                "num" => current_number = tag.attr("numId").map(std::borrow::Cow::into_owned),
                "abstractNumId" => {
                    if let (Some(number), Some(id)) = (current_number.clone(), tag.attr("val")) {
                        numbers.push((number, id.into_owned()));
                    }
                }
                _ => {}
            },
            Event::End("num") => current_number = None,
            _ => {}
        }
    }
    Ok(numbers
        .into_iter()
        .filter_map(|(number, id)| {
            abstracts
                .iter()
                .find(|(abstract_id, _)| *abstract_id == id)
                .map(|(_, levels)| (number, levels.clone()))
        })
        .collect())
}
