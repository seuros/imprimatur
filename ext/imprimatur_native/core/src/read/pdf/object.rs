//! PDF objects (ISO 32000-1, 7.3) and the parser that reads them.

mod name;
#[cfg(test)]
mod tests;

use std::ops::Range;

pub use name::Name;

use super::lexer::{Lexer, Token, hex_bytes, literal_bytes, name_bytes};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    Integer(i64),
    Real(f64),
    Name(Name),
    String(Vec<u8>),
    Array(Vec<Self>),
    Dict(Dict),
    Stream(Stream),
    Ref(u32, u16),
}

/// A dictionary: a handful of keys, so a vector beats a map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dict(pub Vec<(Name, Object)>);

/// A stream: its dictionary and where its raw (encoded, maybe encrypted)
/// bytes lie in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub dict: Dict,
    pub data: Range<usize>,
}

impl Dict {
    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        self.0.iter().find(|(name, _)| **name == *key).map(|(_, value)| value)
    }

    pub fn has_name(&self, key: &[u8], name: &[u8]) -> bool {
        self.get(key).and_then(Object::as_name) == Some(name)
    }
}

impl Object {
    pub const fn as_integer(&self) -> Option<i64> {
        match *self {
            Self::Integer(value) => Some(value),
            _ => None,
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "PDF numbers fit an f64 in practice")]
    pub const fn as_number(&self) -> Option<f64> {
        match *self {
            Self::Integer(value) => Some(value as f64),
            Self::Real(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Self::Name(name) => Some(name),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// A dictionary, or a stream's.
    pub const fn as_dict(&self) -> Option<&Dict> {
        match self {
            Self::Dict(dict) | Self::Stream(Stream { dict, .. }) => Some(dict),
            _ => None,
        }
    }

    pub const fn as_reference(&self) -> Option<(u32, u16)> {
        match *self {
            Self::Ref(number, generation) => Some((number, generation)),
            _ => None,
        }
    }
}

/// Reads objects from a lexer, refusing to nest deeper than `max_depth`.
pub struct Parser<'a> {
    pub lexer: Lexer<'a>,
    max_depth: usize,
}

impl<'a> Parser<'a> {
    pub const fn new(data: &'a [u8], pos: usize, max_depth: usize) -> Self {
        Self { lexer: Lexer::new(data, pos), max_depth }
    }

    /// The next whole object.
    ///
    /// # Errors
    /// [`Error::Malformed`] at the end of the input or on a stray token, and
    /// [`Error::LimitExceeded`] past the nesting limit.
    pub fn object(&mut self) -> Result<Object> {
        let token = self.lexer.next().ok_or_else(|| malformed("an object is cut short"))?;
        self.value(token, 0)
    }

    /// The object a token starts. Integers look ahead for `N G R`.
    pub fn value(&mut self, token: Token<'a>, depth: usize) -> Result<Object> {
        if depth > self.max_depth {
            return Err(Error::LimitExceeded("PDF object nesting depth"));
        }
        Ok(match token {
            Token::Integer(number) => {
                let saved = self.lexer.pos;
                if let (Ok(number), Some(Token::Integer(generation)), Some(Token::Keyword(b"R"))) =
                    (u32::try_from(number), self.lexer.next(), self.lexer.next())
                {
                    Object::Ref(number, u16::try_from(generation).unwrap_or(0))
                } else {
                    self.lexer.pos = saved;
                    Object::Integer(number)
                }
            }
            Token::Real(number) => Object::Real(number),
            Token::Name(raw) => Object::Name(Name::new(&name_bytes(raw))),
            Token::String(raw) => Object::String(literal_bytes(raw).into_owned()),
            Token::Hex(raw) => Object::String(hex_bytes(raw)),
            Token::ArrayStart => {
                let mut items = Vec::new();
                loop {
                    match self.lexer.next() {
                        Some(Token::ArrayEnd) | None => break,
                        Some(token) => items.push(self.value(token, depth + 1)?),
                    }
                }
                Object::Array(items)
            }
            Token::DictStart => Object::Dict(self.dict(depth)?),
            Token::Keyword(b"true") => Object::Bool(true),
            Token::Keyword(b"false") => Object::Bool(false),
            Token::Keyword(_) | Token::ArrayEnd | Token::DictEnd => Object::Null,
        })
    }

    /// A dictionary's entries, after its `<<`. A key without a value, as
    /// broken files have, is dropped.
    fn dict(&mut self, depth: usize) -> Result<Dict> {
        let mut entries = Vec::new();
        loop {
            match self.lexer.next() {
                Some(Token::DictEnd) | None => return Ok(Dict(entries)),
                Some(Token::Name(raw)) => {
                    let saved = self.lexer.pos;
                    match self.lexer.next() {
                        Some(Token::DictEnd) | None => {
                            self.lexer.pos = saved;
                        }
                        Some(token) => {
                            let value = self.value(token, depth + 1)?;
                            entries.push((Name::new(&name_bytes(raw)), value));
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }

    /// An indirect object at the lexer's position: `N G obj ... endobj`,
    /// with its stream data located when it is a stream. `length` resolves
    /// a `/Length` given by reference.
    pub fn indirect(
        &mut self,
        length: &dyn Fn(&Object) -> Option<usize>,
    ) -> Result<(u32, u16, Object)> {
        let header = (self.lexer.next(), self.lexer.next(), self.lexer.next());
        let (
            Some(Token::Integer(number)),
            Some(Token::Integer(generation)),
            Some(Token::Keyword(b"obj")),
        ) = header
        else {
            return Err(malformed("no object where the cross-reference table points"));
        };
        let number = u32::try_from(number).map_err(|_| malformed("a negative object number"))?;
        let generation = u16::try_from(generation).unwrap_or(0);
        let object = self.object()?;
        let Object::Dict(dict) = object else {
            return Ok((number, generation, object));
        };
        let saved = self.lexer.pos;
        if self.lexer.next() != Some(Token::Keyword(b"stream")) {
            self.lexer.pos = saved;
            return Ok((number, generation, Object::Dict(dict)));
        }
        let data = self.lexer.data();
        let mut start = self.lexer.pos;
        if data.get(start) == Some(&b'\r') {
            start += 1;
        }
        if data.get(start) == Some(&b'\n') {
            start += 1;
        }
        let declared = dict.get(b"Length").and_then(length).filter(|&length| {
            let end = start.saturating_add(length);
            data.get(end..).is_some_and(|rest| {
                let rest = &rest
                    [rest.iter().take_while(|&&byte| super::lexer::is_whitespace(byte)).count()..];
                rest.starts_with(b"endstream")
            })
        });
        let end = declared.map_or_else(
            || {
                let found = find(&data[start..], b"endstream").map_or(data.len(), |at| start + at);
                let mut end = found;
                if end > start && data[end - 1] == b'\n' {
                    end -= 1;
                }
                if end > start && data[end - 1] == b'\r' {
                    end -= 1;
                }
                end
            },
            |length| start + length,
        );
        self.lexer.pos = find(&data[end..], b"endstream").map_or(data.len(), |at| end + at + 9);
        Ok((number, generation, Object::Stream(Stream { dict, data: start..end })))
    }
}

/// A `/Length` given directly: what the cross-reference layer, which
/// cannot resolve references yet, passes to [`Parser::indirect`].
pub fn direct_length(length: &Object) -> Option<usize> {
    length.as_integer().and_then(|length| usize::try_from(length).ok())
}

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

pub fn malformed(reason: &str) -> Error {
    Error::malformed("PDF", reason)
}
