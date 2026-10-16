//! `CMaps` (ISO 32000-1, 9.7.5 and 9.10.3): how many bytes a character code
//! takes, which CID it is, and the text it stands for.

#[cfg(test)]
mod tests;

use std::ops::Range;

use super::glyphs::named;
use super::lexer::{Lexer, Token, hex_bytes, name_bytes};
use crate::read::span;

/// Where a code range's text comes from.
#[derive(Debug, Clone)]
enum Target {
    /// One text for one code, or the first of a range whose last
    /// character counts up with the code.
    Text(Range<u32>),
    /// One text per code of the range.
    List(Vec<Range<u32>>),
}

#[derive(Debug, Clone, Default)]
pub struct CMap {
    /// Code space ranges: low, high and length in bytes.
    spaces: Vec<(u32, u32, u8)>,
    /// Code ranges to text, sorted by their low end.
    unicode: Vec<(u32, u32, Target)>,
    /// Code ranges to CIDs: low, high, first CID.
    cids: Vec<(u32, u32, u32)>,
    text: String,
}

impl CMap {
    pub fn parse(data: &[u8]) -> Self {
        let mut cmap = Self::default();
        let mut lexer = Lexer::new(data, 0);
        let mut operands: Vec<Token<'_>> = Vec::new();
        while let Some(token) = lexer.next() {
            let Token::Keyword(keyword) = token else {
                if operands.len() < 1024 {
                    operands.push(token);
                }
                if token == Token::ArrayStart {
                    let items = array(&mut lexer);
                    operands.pop();
                    operands.push(Token::Hex(b""));
                    cmap.push_list(&operands, items);
                }
                continue;
            };
            match keyword {
                b"endcodespacerange" => {
                    for pair in operands.as_chunks::<2>().0 {
                        if let (Some((low, length)), Some((high, _))) =
                            (code(&pair[0]), code(&pair[1]))
                        {
                            cmap.spaces.push((low, high, length));
                        }
                    }
                }
                b"endbfchar" => {
                    for pair in operands.as_chunks::<2>().0 {
                        if let Some((code, _)) = code(&pair[0]) {
                            let target = cmap.target(&pair[1]);
                            cmap.unicode.push((code, code, target));
                        }
                    }
                }
                b"endbfrange" => {
                    for triple in operands.as_chunks::<3>().0 {
                        if let (Some((low, _)), Some((high, _)), Token::Hex(raw)) =
                            (code(&triple[0]), code(&triple[1]), triple[2])
                            && !raw.is_empty()
                        {
                            let target = cmap.target(&triple[2]);
                            cmap.unicode.push((low, high, target));
                        }
                    }
                }
                b"endcidrange" => {
                    for triple in operands.as_chunks::<3>().0 {
                        if let (Some((low, _)), Some((high, _)), Token::Integer(cid)) =
                            (code(&triple[0]), code(&triple[1]), triple[2])
                        {
                            cmap.cids.push((low, high, u32::try_from(cid).unwrap_or(0)));
                        }
                    }
                }
                b"endcidchar" => {
                    for pair in operands.as_chunks::<2>().0 {
                        if let (Some((code, _)), Token::Integer(cid)) = (code(&pair[0]), pair[1]) {
                            cmap.cids.push((code, code, u32::try_from(cid).unwrap_or(0)));
                        }
                    }
                }
                _ => {}
            }
            operands.clear();
        }
        cmap.unicode.sort_by_key(|&(low, _, _)| low);
        cmap.cids.sort_by_key(|&(low, _, _)| low);
        cmap
    }

    /// A `bfrange` whose destination is an array, one text per code.
    fn push_list(&mut self, operands: &[Token<'_>], items: Vec<&[u8]>) {
        let [.., low, high, _] = operands else { return };
        let (Some((low, _)), Some((high, _))) = (code(low), code(high)) else { return };
        let list = items.into_iter().map(|raw| self.store(&utf16(&hex_bytes(raw)))).collect();
        self.unicode.push((low, high, Target::List(list)));
    }

    fn target(&mut self, token: &Token<'_>) -> Target {
        let text = match *token {
            Token::Hex(raw) => utf16(&hex_bytes(raw)),
            Token::Name(raw) => std::str::from_utf8(&name_bytes(raw))
                .ok()
                .and_then(named)
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default(),
            _ => String::new(),
        };
        Target::Text(self.store(&text))
    }

    fn store(&mut self, text: &str) -> Range<u32> {
        span::append(&mut self.text, |buffer| buffer.push_str(text))
    }

    pub const fn has_spaces(&self) -> bool {
        !self.spaces.is_empty()
    }

    /// The length of the code starting `bytes`: the code space range it
    /// falls in, or the shortest one when it falls in none.
    pub fn code_length(&self, bytes: &[u8]) -> usize {
        for &(low, high, length) in &self.spaces {
            let length = usize::from(length);
            if let Some(prefix) = bytes.get(..length)
                && (low..=high).contains(&be(prefix))
            {
                return length;
            }
        }
        self.spaces.iter().map(|&(_, _, length)| usize::from(length)).min().unwrap_or(1)
    }

    /// The text of `code`, pushed onto `out`; false when unmapped.
    pub fn push_text(&self, code: u32, out: &mut String) -> bool {
        let at = self.unicode.partition_point(|&(low, _, _)| low <= code);
        for (low, high, target) in self.unicode[..at].iter().rev().take(8) {
            if code > *high {
                continue;
            }
            let offset = code - low;
            match target {
                Target::Text(span) => {
                    let text = span::slice(&self.text, span);
                    let mut chars = text.chars();
                    let Some(last) = chars.next_back() else { return false };
                    out.push_str(chars.as_str());
                    out.push(char::from_u32(u32::from(last) + offset).unwrap_or(last));
                }
                Target::List(list) => {
                    let Some(span) = list.get(offset as usize) else { return false };
                    out.push_str(span::slice(&self.text, span));
                }
            }
            return true;
        }
        false
    }

    /// The CID of `code`, if the map says.
    pub fn cid(&self, code: u32) -> Option<u32> {
        let at = self.cids.partition_point(|&(low, _, _)| low <= code);
        self.cids[..at]
            .iter()
            .rev()
            .take(8)
            .find(|&&(_, high, _)| code <= high)
            .map(|&(low, _, cid)| cid + (code - low))
    }
}

/// The hex strings of an array after its `[`.
fn array<'a>(lexer: &mut Lexer<'a>) -> Vec<&'a [u8]> {
    let mut items = Vec::new();
    while let Some(token) = lexer.next() {
        match token {
            Token::ArrayEnd => break,
            Token::Hex(raw) => items.push(raw),
            _ => {}
        }
    }
    items
}

/// A hex string token as a code and its length in bytes (up to 4).
fn code(token: &Token<'_>) -> Option<(u32, u8)> {
    let Token::Hex(raw) = *token else { return None };
    let bytes = hex_bytes(raw);
    let length = u8::try_from(bytes.len()).ok().filter(|&length| (1..=4).contains(&length))?;
    Some((be(&bytes), length))
}

pub fn be(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |value, &byte| value << 8 | u32::from(byte))
}

/// UTF-16BE, as `ToUnicode` maps write text; lone surrogates are dropped.
pub fn utf16(bytes: &[u8]) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
    char::decode_utf16(units).filter_map(Result::ok).collect()
}
