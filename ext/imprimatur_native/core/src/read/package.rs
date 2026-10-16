//! The Open Packaging Conventions side of reading: content types, and the
//! relationships that lead from one part to the next.

use std::borrow::Cow;

use super::{Budget, Format};
use crate::xml::reader::{Event, XmlReader, into_text};
use crate::zip::reader::ZipReader;
use crate::{Error, Result};

const OFFICE_DOCUMENT: &str = "officeDocument";

/// A relationship from a part: its id, its type's last segment
/// (`worksheet`, `styles`, `hyperlink`...) and its target, resolved to a
/// part name unless it is external.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub external: bool,
}

pub struct Package<'a> {
    zip: &'a ZipReader<'a>,
    budget: &'a Budget<'a>,
    main: String,
    content_type: String,
}

impl<'a> Package<'a> {
    /// Finds the main part through the package relationships.
    pub fn open(zip: &'a ZipReader<'a>, budget: &'a Budget<'a>) -> Result<Self> {
        let mut package = Self { zip, budget, main: String::new(), content_type: String::new() };
        let main = package
            .relationships("")?
            .into_iter()
            .find(|relationship| relationship.kind == OFFICE_DOCUMENT && !relationship.external)
            .ok_or_else(|| {
                Error::UnsupportedDocument("a zip that is not an Office document".to_owned())
            })?;
        package.content_type = package.content_type_of(&main.target)?;
        package.main = main.target;
        Ok(package)
    }

    pub fn main(&self) -> &str {
        &self.main
    }

    /// What the main part's content type says the document is.
    pub fn format(&self) -> Result<Format> {
        let kind = &self.content_type;
        let unsupported = || Error::UnsupportedDocument(format!("content type {kind}"));
        if kind.contains("binary") {
            return Err(unsupported());
        }
        if kind.contains("spreadsheetml") || kind.contains("ms-excel") {
            Ok(Format::Xlsx)
        } else if kind.contains("wordprocessingml") || kind.contains("ms-word") {
            Ok(Format::Docx)
        } else {
            Err(unsupported())
        }
    }

    /// The main part's text: the workbook, or the document.
    pub fn main_text(&self) -> Result<String> {
        self.text(&self.main)?.ok_or_else(|| {
            let format = if self.format().ok() == Some(Format::Xlsx) { "XLSX" } else { "DOCX" };
            Error::malformed(format, format!("the part {} is missing", self.main))
        })
    }

    /// Parses the part `source` relates to as `kind`, or gives the default
    /// when there is none: styles, shared strings, numbering.
    pub fn parse_related<T: Default>(
        &self,
        relationships: &[Relationship],
        kind: &str,
        parse: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let Some(part) = relationships.iter().find(|r| r.kind == kind && !r.external) else {
            return Ok(T::default());
        };
        self.text(&part.target)?.map_or_else(|| Ok(T::default()), |text| parse(&text))
    }

    /// The parts' names.
    pub fn parts(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.zip.names()
    }

    /// A part's bytes, `None` when the package has no such part.
    pub fn part(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.zip.read(name, self.budget)
    }

    /// The relationships of `source` (`""` for the package's own), in order.
    pub fn relationships(&self, source: &str) -> Result<Vec<Relationship>> {
        let (folder, file) = source.rsplit_once('/').unwrap_or(("", source));
        let rels = if folder.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{folder}/_rels/{file}.rels")
        };
        let Some(text) = self.text(&rels)? else {
            return Ok(Vec::new());
        };
        let mut xml = XmlReader::new(&text, self.budget.max_depth());
        let mut found = Vec::new();
        while let Some(event) = xml.next()? {
            let Event::Start(tag) = event else { continue };
            if tag.name() != "Relationship" {
                continue;
            }
            let (Some(id), Some(kind), Some(target)) =
                (tag.attr("Id"), tag.attr("Type"), tag.attr("Target"))
            else {
                continue;
            };
            let external = tag.attr("TargetMode").is_some_and(|mode| mode == "External");
            let kind = kind.rsplit('/').next().unwrap_or_default().to_owned();
            let target = if external { target.into_owned() } else { resolve(folder, &target) };
            found.push(Relationship { id: id.into_owned(), kind, target, external });
        }
        Ok(found)
    }

    /// The target of `source`'s first relationship of `kind`.
    pub fn related(&self, source: &str, kind: &str) -> Result<Option<String>> {
        Ok(self
            .relationships(source)?
            .into_iter()
            .find(|relationship| relationship.kind == kind && !relationship.external)
            .map(|relationship| relationship.target))
    }

    /// A part as text, `None` when missing.
    pub fn text(&self, name: &str) -> Result<Option<String>> {
        self.part(name)?.map(into_text).transpose()
    }

    fn content_type_of(&self, part: &str) -> Result<String> {
        let text = self
            .text("[Content_Types].xml")?
            .ok_or_else(|| Error::UnsupportedDocument("a zip without content types".to_owned()))?;
        let mut xml = XmlReader::new(&text, self.budget.max_depth());
        let extension = part.rsplit_once('.').map_or("", |(_, extension)| extension);
        let mut default = None;
        while let Some(event) = xml.next()? {
            let Event::Start(tag) = event else { continue };
            match tag.name() {
                "Override"
                    if tag.attr("PartName").is_some_and(|name| {
                        name.trim_start_matches('/').eq_ignore_ascii_case(part)
                    }) =>
                {
                    return Ok(tag.attr("ContentType").unwrap_or_default().into_owned());
                }
                "Default"
                    if tag
                        .attr("Extension")
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) =>
                {
                    default = tag.attr("ContentType").map(Cow::into_owned);
                }
                _ => {}
            }
        }
        Ok(default.unwrap_or_default())
    }
}

/// A relationship target as a part name: relative to the source's folder,
/// or absolute with a leading `/`.
fn resolve(folder: &str, target: &str) -> String {
    let (base, target) =
        target.strip_prefix('/').map_or((folder, target), |absolute| ("", absolute));
    let mut segments: Vec<&str> = base.split('/').filter(|segment| !segment.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}
