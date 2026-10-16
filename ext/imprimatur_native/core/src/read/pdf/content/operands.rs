//! Content stream operands, borrowed from the stream: reading a page
//! allocates nothing per operator. `TJ`'s arrays keep their items in one
//! buffer shared by the whole stream.

use crate::read::pdf::lexer::{Lexer, Token, is_whitespace, name_bytes};
use crate::read::pdf::object::{Dict, Name, Object, Parser};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operand<'c> {
    Number(f64),
    /// A name as written, `#xx` escapes and all.
    Name(&'c [u8]),
    /// A string as written: a literal's bytes between its parentheses, or
    /// a hexadecimal string's digits.
    String {
        raw: &'c [u8],
        hex: bool,
    },
    /// An array of numbers and strings: where its items start and end in
    /// the buffer.
    Array(usize, usize),
    /// Anything no operator here reads: dictionaries, nested arrays.
    Other,
}

/// The operand a token starts; arrays fill `items`, dictionaries are skipped.
pub fn operand<'c>(
    token: Token<'c>,
    lexer: &mut Lexer<'c>,
    items: &mut Vec<Operand<'c>>,
) -> Operand<'c> {
    match token {
        Token::ArrayStart => {
            let start = items.len();
            while let Some(token) = lexer.next() {
                match token {
                    Token::ArrayEnd => break,
                    Token::ArrayStart | Token::DictStart => skip(lexer),
                    token => match scalar(token) {
                        Operand::Other => {}
                        item => items.push(item),
                    },
                }
            }
            Operand::Array(start, items.len())
        }
        Token::DictStart => {
            skip(lexer);
            Operand::Other
        }
        token => scalar(token),
    }
}

#[expect(clippy::cast_precision_loss, reason = "content stream numbers are small")]
const fn scalar(token: Token<'_>) -> Operand<'_> {
    match token {
        Token::Integer(number) => Operand::Number(number as f64),
        Token::Real(number) => Operand::Number(number),
        Token::Name(raw) => Operand::Name(raw),
        Token::String(raw) => Operand::String { raw, hex: false },
        Token::Hex(raw) => Operand::String { raw, hex: true },
        _ => Operand::Other,
    }
}

/// Skips a nested array or dictionary, its opening token already read.
fn skip(lexer: &mut Lexer<'_>) {
    let mut depth = 1_usize;
    while depth > 0 {
        match lexer.next() {
            None => return,
            Some(Token::ArrayStart | Token::DictStart) => depth += 1,
            Some(Token::ArrayEnd | Token::DictEnd) => depth -= 1,
            Some(_) => {}
        }
    }
}

/// An inline image after its `BI`: its dictionary, abbreviations spelled
/// out, and its data up to `EI`. The lexer moves past it either way.
pub fn inline_image<'c>(lexer: &mut Lexer<'c>, max_depth: usize) -> Option<(Dict, &'c [u8])> {
    let data = lexer.data();
    let mut parser = Parser::new(data, lexer.pos, max_depth);
    let mut entries = Vec::new();
    let found = loop {
        match parser.lexer.next() {
            None => break false,
            Some(Token::Keyword(b"ID")) => break true,
            Some(Token::Name(key)) => {
                let Some(token) = parser.lexer.next() else { break false };
                if token == Token::Keyword(b"ID") {
                    break true;
                }
                let Ok(value) = parser.value(token, 0) else { break false };
                entries.push((Name::new(full_key(&name_bytes(key))), full_value(value)));
            }
            Some(_) => {}
        }
    };
    // One whitespace byte separates `ID` from the data.
    let start = (parser.lexer.pos + 1).min(data.len());
    let mut at = start;
    while at + 2 <= data.len() {
        if &data[at..at + 2] == b"EI"
            && data.get(at.wrapping_sub(1)).is_some_and(|&byte| is_whitespace(byte))
            && data.get(at + 2).is_none_or(|&byte| is_whitespace(byte))
        {
            lexer.pos = at + 2;
            let end = if at > start { at - 1 } else { at };
            return found.then(|| (Dict(entries), &data[start..end]));
        }
        at += 1;
    }
    lexer.pos = data.len();
    None
}

/// Inline images abbreviate their keys (ISO 32000-1, Table 93).
const fn full_key(key: &[u8]) -> &[u8] {
    match key {
        b"BPC" => b"BitsPerComponent",
        b"CS" => b"ColorSpace",
        b"D" => b"Decode",
        b"DP" => b"DecodeParms",
        b"F" => b"Filter",
        b"H" => b"Height",
        b"IM" => b"ImageMask",
        b"I" => b"Interpolate",
        b"W" => b"Width",
        b"L" => b"Length",
        key => key,
    }
}

/// And their names (Table 94): colour spaces and filters.
fn full_value(value: Object) -> Object {
    match value {
        Object::Name(name) => Object::Name(match &*name {
            b"G" => Name::new(b"DeviceGray"),
            b"RGB" => Name::new(b"DeviceRGB"),
            b"CMYK" => Name::new(b"DeviceCMYK"),
            b"I" => Name::new(b"Indexed"),
            b"AHx" => Name::new(b"ASCIIHexDecode"),
            b"A85" => Name::new(b"ASCII85Decode"),
            b"LZW" => Name::new(b"LZWDecode"),
            b"Fl" => Name::new(b"FlateDecode"),
            b"RL" => Name::new(b"RunLengthDecode"),
            b"CCF" => Name::new(b"CCITTFaxDecode"),
            b"DCT" => Name::new(b"DCTDecode"),
            _ => name,
        }),
        Object::Array(items) => Object::Array(items.into_iter().map(full_value).collect()),
        value => value,
    }
}
