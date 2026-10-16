//! Cell text: the OOXML escaping for characters XML cannot carry, and the
//! shared string table.

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;

use super::package::MAIN_NS;
use crate::xml::{XmlWriter, is_forbidden};
use crate::{Error, Result};

/// Characters a cell or a comment holds at most.
const MAX_TEXT: usize = 32_767;

/// Refuses text longer than a cell or a comment holds.
pub fn check_length(text: &str) -> Result<()> {
    let length = text.chars().count();
    (length <= MAX_TEXT).ok_or(Error::TextTooLong(length))
}

/// Escapes text as an `ST_Xstring` (ECMA-376 Part 1, 22.9.2.19): characters
/// XML cannot carry become `_xHHHH_`, and an underscore that would read as the
/// start of such an escape becomes `_x005F_`.
pub fn escape(text: &str) -> Cow<'_, str> {
    let needs = |(at, ch): (usize, char)| is_forbidden(ch) || looks_escaped(text, at);
    if !text.char_indices().any(needs) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 7);
    for (at, ch) in text.char_indices() {
        if needs((at, ch)) {
            // Writing to a String cannot fail.
            let _ = write!(escaped, "_x{:04X}_", u32::from(ch));
        } else {
            escaped.push(ch);
        }
    }
    Cow::Owned(escaped)
}

/// The inverse of [`escape`]: each `_xHHHH_` back to its character. An
/// escape of a surrogate, which no `char` holds, stays as written.
pub fn unescape(text: &str) -> Cow<'_, str> {
    if !text.contains("_x") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("_x") {
        out.push_str(&rest[..at]);
        let candidate = &rest[at..];
        let decoded = looks_escaped(candidate, 0)
            .then(|| u32::from_str_radix(&candidate[2..6], 16).ok().and_then(char::from_u32))
            .flatten();
        if let Some(ch) = decoded {
            out.push(ch);
            rest = &candidate[7..];
        } else {
            out.push_str("_x");
            rest = &candidate[2..];
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// True when byte `at` starts `_xHHHH_`. The pattern is ASCII, so checking
/// bytes from `at` is exact and keeps escaping linear.
fn looks_escaped(text: &str, at: usize) -> bool {
    let Some([b'_', b'x', digits @ .., b'_']) = text.as_bytes().get(at..at + 7) else {
        return false;
    };
    digits.iter().all(u8::is_ascii_hexdigit)
}

/// Whether `<t>` needs `xml:space="preserve"` to keep its edge whitespace.
pub fn needs_preserve(text: &str) -> bool {
    text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace)
}

/// Every distinct text in the workbook, numbered in order of first use.
#[derive(Default)]
pub struct SharedStrings<'a> {
    index: HashMap<&'a str, usize>,
    order: Vec<&'a str>,
    references: usize,
}

impl<'a> SharedStrings<'a> {
    /// The text's index in the table, adding it on first use.
    pub fn intern(&mut self, text: &'a str) -> usize {
        self.references += 1;
        *self.index.entry(text).or_insert_with(|| {
            self.order.push(text);
            self.order.len() - 1
        })
    }

    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// `xl/sharedStrings.xml`.
    pub fn to_xml(&self) -> String {
        let mut xml = XmlWriter::new();
        let count = self.references.to_string();
        let unique = self.order.len().to_string();
        xml.open("sst", &[("xmlns", MAIN_NS), ("count", &count), ("uniqueCount", &unique)]);
        for text in &self.order {
            xml.open("si", &[]);
            let attrs: &[(&str, &str)] =
                if needs_preserve(text) { &[("xml:space", "preserve")] } else { &[] };
            xml.element("t", attrs, &escape(text));
            xml.close("si");
        }
        xml.close("sst");
        xml.finish()
    }
}
