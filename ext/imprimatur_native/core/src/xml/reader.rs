//! A pull parser for the XML parts of OOXML packages. It reads untrusted
//! input, so it is deliberately small: no DTDs (refused outright, which rules
//! out entity expansion attacks), only the five predefined entities and
//! character references, and a nesting limit. Names, attributes and text
//! borrow from the input; only text with an entity in it is copied.
//!
//! Element and attribute names are matched by their local part: `w:p` is
//! `p`. OOXML parts use one namespace per prefix in practice.

#[cfg(test)]
mod tests;

use std::borrow::Cow;

use crate::{Error, Result};

/// One step through the document. A self-closing element is a `Start`
/// followed by its `End`, so readers need not tell the two forms apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<'a> {
    Start(Tag<'a>),
    End(&'a str),
    Text(Cow<'a, str>),
}

/// An opening tag: its local name and its raw, still escaped, attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag<'a> {
    name: &'a str,
    attrs: &'a str,
}

impl<'a> Tag<'a> {
    /// A tag of this name with these raw attributes, as they sit between
    /// the name and the closing `>`.
    pub const fn new(name: &'a str, attrs: &'a str) -> Self {
        Self { name, attrs }
    }

    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// The attribute with this local name, unescaped.
    pub fn attr(&self, local: &str) -> Option<Cow<'a, str>> {
        let mut rest = self.attrs;
        loop {
            rest = rest.trim_start();
            let (name, after) = rest.split_once('=')?;
            let after = after.trim_start();
            let quote = after.chars().next().filter(|&quote| quote == '"' || quote == '\'')?;
            let (value, tail) = after[1..].split_once(quote)?;
            if local_name(name.trim_end()) == local {
                return unescape(value).ok();
            }
            rest = tail;
        }
    }

    /// `w:val`-style booleans: present without a value, `1`, `true` or
    /// `on` are true; Word writes `w:val="0"` to switch a property off.
    pub fn flag(&self, local: &str) -> bool {
        self.attr(local).is_none_or(|value| matches!(&*value, "1" | "true" | "on"))
    }
}

pub struct XmlReader<'a> {
    text: &'a str,
    pos: usize,
    depth: usize,
    max_depth: usize,
    /// The `End` a self-closing element still owes.
    pending_end: Option<&'a str>,
}

impl<'a> XmlReader<'a> {
    pub const fn new(text: &'a str, max_depth: usize) -> Self {
        Self { text, pos: 0, depth: 0, max_depth, pending_end: None }
    }

    /// The next event, or `None` at the end of the input.
    ///
    /// # Errors
    /// [`Error::Malformed`] for broken markup or a DTD, and
    /// [`Error::LimitExceeded`] beyond the nesting limit.
    pub fn next(&mut self) -> Result<Option<Event<'a>>> {
        if let Some(name) = self.pending_end.take() {
            self.depth -= 1;
            return Ok(Some(Event::End(name)));
        }
        loop {
            let rest = &self.text[self.pos..];
            if rest.is_empty() {
                return Ok(None);
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.pos += end;
                return Ok(Some(Event::Text(unescape(&rest[..end])?)));
            }
            if let Some(body) = rest.strip_prefix("<?") {
                self.pos += 2 + find(body, "?>")? + 2;
            } else if let Some(body) = rest.strip_prefix("<!--") {
                self.pos += 4 + find(body, "-->")? + 3;
            } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
                let end = find(body, "]]>")?;
                self.pos += 9 + end + 3;
                return Ok(Some(Event::Text(Cow::Borrowed(&body[..end]))));
            } else if rest.starts_with("<!") {
                return Err(malformed("a DTD or declaration, which is not allowed"));
            } else if let Some(body) = rest.strip_prefix("</") {
                let end = find(body, ">")?;
                self.pos += 2 + end + 1;
                self.depth =
                    self.depth.checked_sub(1).ok_or_else(|| malformed("an extra end tag"))?;
                return Ok(Some(Event::End(local_name(body[..end].trim_end()))));
            } else {
                return self.start(&rest[1..]).map(Some);
            }
        }
    }

    /// Skips to the end of the element whose `Start` was just read.
    pub fn skip(&mut self) -> Result<()> {
        let mut open = 1_usize;
        while open > 0 {
            match self.next()? {
                Some(Event::Start(_)) => open += 1,
                Some(Event::End(_)) => open -= 1,
                Some(Event::Text(_)) => {}
                None => return Err(malformed("an element that never ends")),
            }
        }
        Ok(())
    }

    fn start(&mut self, body: &'a str) -> Result<Event<'a>> {
        let end = tag_end(body)?;
        let (inner, empty) =
            body[..end].strip_suffix('/').map_or((&body[..end], false), |inner| (inner, true));
        let name_end = inner.find(|ch: char| ch.is_ascii_whitespace()).unwrap_or(inner.len());
        let name = &inner[..name_end];
        if name.is_empty() {
            return Err(malformed("a tag without a name"));
        }
        self.pos += 1 + end + 1;
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(Error::LimitExceeded("XML nesting depth"));
        }
        let name = local_name(name);
        if empty {
            self.pending_end = Some(name);
        }
        Ok(Event::Start(Tag { name, attrs: &inner[name_end..] }))
    }
}

/// A part's text: UTF-8, with or without a byte order mark, or UTF-16
/// with one. UTF-8 takes over the bytes' allocation.
pub fn into_text(mut bytes: Vec<u8>) -> Result<String> {
    let utf16 = |bytes: &[u8], from: fn([u8; 2]) -> u16| {
        let units = bytes.as_chunks::<2>().0.iter().map(|pair| from([pair[0], pair[1]]));
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| malformed("invalid UTF-16"))
    };
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => return utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => return utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, ..] => {
            bytes.drain(..3);
        }
        _ => {}
    }
    String::from_utf8(bytes).map_err(|_| malformed("invalid UTF-8"))
}

/// Replaces the predefined entities and character references.
pub fn unescape(text: &str) -> Result<Cow<'_, str>> {
    let Some(first) = text.find('&') else {
        return Ok(Cow::Borrowed(text));
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..first]);
    let mut rest = &text[first..];
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let entity = &rest[at + 1..];
        let end = entity.find(';').filter(|&end| end <= 10).ok_or_else(|| malformed("a bare &"))?;
        out.push(match &entity[..end] {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            reference => character_reference(reference)?,
        });
        rest = &entity[end + 1..];
    }
    out.push_str(rest);
    Ok(Cow::Owned(out))
}

fn character_reference(reference: &str) -> Result<char> {
    let code = match reference.strip_prefix('#') {
        Some(hex) if hex.starts_with(['x', 'X']) => u32::from_str_radix(&hex[1..], 16).ok(),
        Some(decimal) => decimal.parse().ok(),
        None => None,
    };
    code.and_then(char::from_u32).ok_or_else(|| malformed("an unknown entity"))
}

/// The end of a start tag: its `>`, outside quoted attribute values.
fn tag_end(body: &str) -> Result<usize> {
    let mut quote = None;
    for (at, byte) in body.bytes().enumerate() {
        match (quote, byte) {
            (None, b'>') => return Ok(at),
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), _) if open == byte => quote = None,
            _ => {}
        }
    }
    Err(malformed("an unterminated tag"))
}

fn find(haystack: &str, needle: &str) -> Result<usize> {
    haystack.find(needle).ok_or_else(|| malformed("unterminated markup"))
}

pub fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn malformed(reason: &str) -> Error {
    Error::malformed("XML", reason)
}
