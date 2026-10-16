//! PDF tokens (ISO 32000-1, 7.2), for the file's objects and for content
//! streams alike. Tokens borrow from the input; strings and names are
//! decoded only when asked.

#[cfg(test)]
mod tests;

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Token<'a> {
    Integer(i64),
    Real(f64),
    /// A name without its slash, `#xx` escapes still in place.
    Name(&'a [u8]),
    /// A literal string's bytes between its parentheses, escapes in place.
    String(&'a [u8]),
    /// A hexadecimal string's digits between `<` and `>`.
    Hex(&'a [u8]),
    ArrayStart,
    ArrayEnd,
    DictStart,
    DictEnd,
    /// `true`, `obj`, `R`, `Tj`...: anything else made of regular characters.
    Keyword(&'a [u8]),
}

pub const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

pub const fn is_delimiter(byte: u8) -> bool {
    matches!(byte, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

const fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

#[derive(Clone)]
pub struct Lexer<'a> {
    pub pos: usize,
    data: &'a [u8],
}

impl<'a> Lexer<'a> {
    pub const fn new(data: &'a [u8], pos: usize) -> Self {
        Self { pos, data }
    }

    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Skips whitespace and comments.
    pub fn skip_space(&mut self) {
        while let Some(&byte) = self.data.get(self.pos) {
            if is_whitespace(byte) {
                self.pos += 1;
            } else if byte == b'%' {
                while self.data.get(self.pos).is_some_and(|&byte| byte != b'\n' && byte != b'\r') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next token, `None` at the end. Stray closing delimiters are
    /// skipped: broken files have them, and nothing reads them.
    pub fn next(&mut self) -> Option<Token<'a>> {
        loop {
            self.skip_space();
            let &byte = self.data.get(self.pos)?;
            self.pos += 1;
            return Some(match byte {
                b'[' => Token::ArrayStart,
                b']' => Token::ArrayEnd,
                b'<' if self.data.get(self.pos) == Some(&b'<') => {
                    self.pos += 1;
                    Token::DictStart
                }
                b'>' if self.data.get(self.pos) == Some(&b'>') => {
                    self.pos += 1;
                    Token::DictEnd
                }
                b'<' => {
                    let start = self.pos;
                    let end = self.data[start..]
                        .iter()
                        .position(|&byte| byte == b'>')
                        .map_or(self.data.len(), |at| start + at);
                    self.pos = (end + 1).min(self.data.len());
                    Token::Hex(&self.data[start..end])
                }
                b'(' => self.literal(),
                b'/' => {
                    let start = self.pos;
                    self.regular();
                    Token::Name(&self.data[start..self.pos])
                }
                b')' | b'>' | b'{' | b'}' => continue,
                _ => {
                    let start = self.pos - 1;
                    self.regular();
                    let word = &self.data[start..self.pos];
                    number(word).unwrap_or(Token::Keyword(word))
                }
            });
        }
    }

    fn regular(&mut self) {
        while self.data.get(self.pos).is_some_and(|&byte| is_regular(byte)) {
            self.pos += 1;
        }
    }

    /// A literal string: parentheses balance unless escaped.
    fn literal(&mut self) -> Token<'a> {
        let start = self.pos;
        let mut depth = 1_usize;
        while let Some(&byte) = self.data.get(self.pos) {
            self.pos += 1;
            match byte {
                b'\\' => self.pos += 1,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Token::String(&self.data[start..self.pos - 1]);
                    }
                }
                _ => {}
            }
        }
        self.pos = self.data.len();
        Token::String(&self.data[start..])
    }
}

/// An integer or a real (`+1`, `-.5`, `3.`), or `None` for anything else.
fn number(word: &[u8]) -> Option<Token<'static>> {
    let first = *word.first()?;
    if !(first.is_ascii_digit() || matches!(first, b'+' | b'-' | b'.')) {
        return None;
    }
    let text = std::str::from_utf8(word).ok()?;
    if let Ok(integer) = text.parse::<i64>() {
        return Some(Token::Integer(integer));
    }
    let digits = text.trim_start_matches(['+', '-']);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.') {
        // `--5` and the like: Acrobat reads the number, so do we.
        return digits.trim_start_matches('-').parse::<f64>().ok().map(Token::Real);
    }
    let value = if digits == "." { 0.0 } else { digits.parse::<f64>().ok()? };
    Some(Token::Real(if text.starts_with('-') { -value } else { value }))
}

/// A literal string's bytes with its escapes resolved (ISO 32000-1, 7.3.4.2).
pub fn literal_bytes(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.iter().any(|&byte| byte == b'\\' || byte == b'\r') {
        return Cow::Borrowed(raw);
    }
    let mut out = Vec::with_capacity(raw.len());
    literal_into(raw, &mut out);
    Cow::Owned(out)
}

/// As [`literal_bytes`], appended to `out`.
pub fn literal_into(raw: &[u8], out: &mut Vec<u8>) {
    let mut bytes = raw.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\r' => {
                bytes.next_if_eq(&b'\n');
                out.push(b'\n');
            }
            b'\\' => match bytes.next() {
                Some(b'n') => out.push(b'\n'),
                Some(b'r') => out.push(b'\r'),
                Some(b't') => out.push(b'\t'),
                Some(b'b') => out.push(0x08),
                Some(b'f') => out.push(0x0C),
                Some(b'\r') => {
                    bytes.next_if_eq(&b'\n');
                }
                Some(digit @ b'0'..=b'7') => {
                    let mut value = u32::from(digit - b'0');
                    for _ in 0..2 {
                        let Some(next) = bytes.next_if(|byte| (b'0'..=b'7').contains(byte)) else {
                            break;
                        };
                        value = value * 8 + u32::from(next - b'0');
                    }
                    out.push(value.to_le_bytes()[0]);
                }
                Some(b'\n') | None => {}
                Some(other) => out.push(other),
            },
            byte => out.push(byte),
        }
    }
}

/// A hexadecimal string's bytes; whitespace is ignored and a final odd
/// digit counts as followed by 0.
pub fn hex_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() / 2 + 1);
    hex_into(raw, &mut out);
    out
}

/// As [`hex_bytes`], appended to `out`.
pub fn hex_into(raw: &[u8], out: &mut Vec<u8>) {
    let mut high = None;
    for digit in raw.iter().filter_map(|&byte| char::from(byte).to_digit(16)) {
        let digit = digit.to_le_bytes()[0];
        match high.take() {
            Some(high) => out.push(high << 4 | digit),
            None => high = Some(digit),
        }
    }
    if let Some(high) = high {
        out.push(high << 4);
    }
}

/// A name's bytes with its `#xx` escapes resolved.
pub fn name_bytes(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.contains(&b'#') {
        return Cow::Borrowed(raw);
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let escaped = raw
            .get(at + 1..at + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (raw[at], escaped) {
            (b'#', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    Cow::Owned(out)
}
