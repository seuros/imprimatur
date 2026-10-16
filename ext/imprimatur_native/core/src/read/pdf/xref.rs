//! Where each object lives (ISO 32000-1, 7.5.4 and 7.5.8): cross-reference
//! tables and streams, followed back through `/Prev` and `/XRefStm`. When
//! they are missing or wrong, as in damaged files, the table is rebuilt by
//! scanning the file for `N G obj`.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use super::filter;
use super::lexer::{Lexer, Token};
use super::object::{Dict, Name, Object, Parser, direct_length, find};
use crate::Result;
use crate::read::Budget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Free,
    Offset { offset: usize, generation: u16 },
    Compressed { stream: u32, index: u32 },
}

#[derive(Debug, Default)]
pub struct Xref {
    pub entries: HashMap<u32, Entry>,
    pub trailer: Dict,
}

impl Xref {
    /// The table the file declares, or one rebuilt from its objects when
    /// that has no `/Root`.
    pub fn read(data: &[u8], budget: &Budget<'_>) -> Result<Self> {
        let mut xref = Self::default();
        let declared = start(data).map(|offset| xref.follow(data, offset, budget));
        match declared {
            Some(Ok(())) if xref.trailer.get(b"Root").is_some() && !xref.entries.is_empty() => {
                Ok(xref)
            }
            Some(Err(error)) if is_limit(&error) => Err(error),
            _ => Self::rebuild(data, budget),
        }
    }

    /// Reads the section at `offset` and those before it. Newer sections
    /// come first, so an object already known keeps its entry, except one
    /// a hybrid file's table marks free for older readers.
    fn follow(&mut self, data: &[u8], offset: usize, budget: &Budget<'_>) -> Result<()> {
        let mut pending = vec![offset];
        let mut seen = Vec::new();
        while let Some(offset) = pending.pop() {
            if seen.contains(&offset) || seen.len() > 1_000 {
                continue;
            }
            seen.push(offset);
            budget.check()?;
            let mut lexer = Lexer::new(data, offset);
            lexer.skip_space();
            let trailer = if data.get(lexer.pos..).is_some_and(|rest| rest.starts_with(b"xref")) {
                lexer.pos += 4;
                self.table(&mut lexer, budget)?
            } else {
                self.stream(data, lexer.pos, budget)?
            };
            for key in [&b"Prev"[..], b"XRefStm"] {
                if let Some(at) = trailer
                    .get(key)
                    .and_then(Object::as_integer)
                    .and_then(|at| usize::try_from(at).ok())
                {
                    pending.push(at);
                }
            }
            for (key, value) in trailer.0 {
                if self.trailer.get(&key).is_none() {
                    self.trailer.0.push((key, value));
                }
            }
        }
        Ok(())
    }

    fn insert(&mut self, number: u32, entry: Entry) {
        let slot = self.entries.entry(number).or_insert(entry);
        if *slot == Entry::Free {
            *slot = entry;
        }
    }

    /// A classic table after its `xref` keyword, then its trailer.
    fn table(&mut self, lexer: &mut Lexer<'_>, budget: &Budget<'_>) -> Result<Dict> {
        loop {
            let saved = lexer.pos;
            match (lexer.next(), lexer.next()) {
                (Some(Token::Integer(first)), Some(Token::Integer(count))) => {
                    let (Ok(first), Ok(count)) = (u32::try_from(first), u32::try_from(count))
                    else {
                        break;
                    };
                    for number in first..first.saturating_add(count) {
                        budget.tick()?;
                        let (
                            Some(Token::Integer(offset)),
                            Some(Token::Integer(generation)),
                            Some(Token::Keyword(kind)),
                        ) = (lexer.next(), lexer.next(), lexer.next())
                        else {
                            break;
                        };
                        let entry = match (kind, usize::try_from(offset)) {
                            (b"n", Ok(offset)) if offset > 0 => Entry::Offset {
                                offset,
                                generation: u16::try_from(generation).unwrap_or(0),
                            },
                            _ => Entry::Free,
                        };
                        self.insert(number, entry);
                    }
                }
                (Some(Token::Keyword(b"trailer")), Some(Token::DictStart)) => {
                    lexer.pos = saved;
                    lexer.next();
                    let mut parser = Parser::new(lexer.data(), lexer.pos, budget.max_depth());
                    return Ok(parser.object()?.as_dict().cloned().unwrap_or_default());
                }
                _ => break,
            }
        }
        Ok(Dict::default())
    }

    /// A cross-reference stream at `offset`; its dictionary is the trailer.
    fn stream(&mut self, data: &[u8], offset: usize, budget: &Budget<'_>) -> Result<Dict> {
        let mut parser = Parser::new(data, offset, budget.max_depth());
        let (_, _, object) = parser.indirect(&direct_length)?;
        let Object::Stream(stream) = object else { return Ok(Dict::default()) };
        let widths: Vec<usize> = stream
            .dict
            .get(b"W")
            .and_then(Object::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|width| width.as_integer().and_then(|width| usize::try_from(width).ok()))
            .collect();
        if widths.len() < 3 || widths.iter().any(|&width| width > 8) {
            return Ok(stream.dict);
        }
        let filters = filters_of(&stream.dict);
        let decoded = filter::decode(&data[stream.data.clone()], &filters, budget)?;
        let size = stream.dict.get(b"Size").and_then(Object::as_integer).unwrap_or(0);
        let index: Vec<i64> = stream.dict.get(b"Index").and_then(Object::as_array).map_or_else(
            || vec![0, size],
            |index| index.iter().filter_map(Object::as_integer).collect(),
        );
        let row = widths.iter().sum::<usize>().max(1);
        let mut rows = decoded.chunks_exact(row);
        for pair in index.as_chunks::<2>().0 {
            let (Ok(first), Ok(count)) = (u32::try_from(pair[0]), u32::try_from(pair[1])) else {
                continue;
            };
            for number in first..first.saturating_add(count) {
                let Some(row) = rows.next() else { break };
                budget.tick()?;
                let (kind, rest) = row.split_at(widths[0]);
                let (second, third) = rest.split_at(widths[1]);
                let kind = if widths[0] == 0 { 1 } else { be(kind) };
                let entry = match kind {
                    1 => Entry::Offset {
                        offset: usize::try_from(be(second)).unwrap_or(0),
                        generation: u16::try_from(be(third)).unwrap_or(0),
                    },
                    2 => Entry::Compressed {
                        stream: u32::try_from(be(second)).unwrap_or(u32::MAX),
                        index: u32::try_from(be(third)).unwrap_or(0),
                    },
                    _ => Entry::Free,
                };
                self.insert(number, entry);
            }
        }
        Ok(stream.dict)
    }

    /// Rebuilds the table from every `N G obj` in the file, later ones
    /// winning as incremental updates do; the trailer from every `trailer`
    /// dictionary, or from the catalog when there is none.
    fn rebuild(data: &[u8], budget: &Budget<'_>) -> Result<Self> {
        let mut xref = Self::default();
        let mut at = 0;
        while let Some(found) = find(&data[at..], b"obj") {
            budget.tick()?;
            let keyword = at + found;
            at = keyword + 3;
            if let Some((number, generation, offset)) = header_before(data, keyword) {
                xref.entries.insert(number, Entry::Offset { offset, generation });
            }
        }
        let mut at = 0;
        while let Some(found) = find(&data[at..], b"trailer") {
            at += found + 7;
            let mut parser = Parser::new(data, at, budget.max_depth());
            if let Ok(Object::Dict(trailer)) = parser.object() {
                for (key, value) in trailer.0 {
                    xref.trailer.0.retain(|(existing, _)| *existing != key);
                    xref.trailer.0.push((key, value));
                }
            }
        }
        xref.register_object_streams(data, budget)?;
        if xref.trailer.get(b"Root").is_none() {
            for (number, offset) in xref.offsets() {
                let mut parser = Parser::new(data, offset, budget.max_depth());
                if let Ok((_, generation, Object::Dict(dict))) = parser.indirect(&|_| None)
                    && dict.has_name(b"Type", b"Catalog")
                {
                    xref.trailer.0.push((Name::new(b"Root"), Object::Ref(number, generation)));
                    break;
                }
            }
        }
        Ok(xref)
    }
}

impl Xref {
    /// The objects stored in the file itself, with their offsets.
    fn offsets(&self) -> Vec<(u32, usize)> {
        self.entries
            .iter()
            .filter_map(|(&number, entry)| match *entry {
                Entry::Offset { offset, .. } => Some((number, offset)),
                _ => None,
            })
            .collect()
    }

    /// After a rebuild: the objects inside object streams, which the scan
    /// cannot see, by the stream's own index of them.
    fn register_object_streams(&mut self, data: &[u8], budget: &Budget<'_>) -> Result<()> {
        let streams = self.offsets();
        for (stream_number, offset) in streams {
            budget.tick()?;
            let mut parser = Parser::new(data, offset, budget.max_depth());
            let Ok((_, _, Object::Stream(stream))) = parser.indirect(&direct_length) else {
                continue;
            };
            if !stream.dict.has_name(b"Type", b"ObjStm") {
                continue;
            }
            let Ok(decoded) =
                filter::decode(&data[stream.data.clone()], &filters_of(&stream.dict), budget)
            else {
                continue;
            };
            let count = stream.dict.get(b"N").and_then(Object::as_integer).unwrap_or(0);
            let mut lexer = Lexer::new(&decoded, 0);
            for index in 0..u32::try_from(count).unwrap_or(0) {
                let (Some(Token::Integer(number)), Some(Token::Integer(_))) =
                    (lexer.next(), lexer.next())
                else {
                    break;
                };
                if let Ok(number) = u32::try_from(number) {
                    self.entries
                        .entry(number)
                        .or_insert(Entry::Compressed { stream: stream_number, index });
                }
            }
        }
        Ok(())
    }
}

/// `N G` right before an `obj` keyword at `keyword`: the object's number,
/// generation and offset.
fn header_before(data: &[u8], keyword: usize) -> Option<(u32, u16, usize)> {
    let before = &data[..keyword];
    let generation_end = before.iter().rposition(|&byte| !super::lexer::is_whitespace(byte))? + 1;
    if generation_end == keyword {
        return None;
    }
    let generation_start = before[..generation_end]
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())
        .map_or(0, |at| at + 1);
    let number_end =
        before[..generation_start].iter().rposition(|&byte| !super::lexer::is_whitespace(byte))?
            + 1;
    let number_start =
        before[..number_end].iter().rposition(|byte| !byte.is_ascii_digit()).map_or(0, |at| at + 1);
    if generation_start == generation_end
        || number_start == number_end
        || number_end == generation_start
    {
        return None;
    }
    let digits =
        |range: std::ops::Range<usize>| std::str::from_utf8(&data[range]).ok()?.parse::<u32>().ok();
    Some((
        digits(number_start..number_end)?,
        u16::try_from(digits(generation_start..generation_end)?).ok()?,
        number_start,
    ))
}

/// The offset after the last `startxref`.
fn start(data: &[u8]) -> Option<usize> {
    let tail = data.len().saturating_sub(4096);
    let found = data[tail..].windows(9).rposition(|window| window == b"startxref")?;
    let mut lexer = Lexer::new(data, tail + found + 9);
    match lexer.next()? {
        Token::Integer(offset) => {
            usize::try_from(offset).ok().filter(|&offset| offset < data.len())
        }
        _ => None,
    }
}

/// A stream's filters with their parameters, as written in its dictionary
/// (direct objects only, which is all cross-reference streams may use).
pub fn filters_of(dict: &Dict) -> Vec<(&[u8], Option<&Dict>)> {
    let params = dict.get(b"DecodeParms");
    match dict.get(b"Filter") {
        Some(Object::Name(name)) => vec![(&**name, params.and_then(Object::as_dict))],
        Some(Object::Array(names)) => names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let params = params
                    .and_then(Object::as_array)
                    .and_then(|params| params.get(index))
                    .and_then(Object::as_dict);
                Some((name.as_name()?, params))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |value, &byte| value << 8 | u64::from(byte))
}

pub const fn is_limit(error: &crate::Error) -> bool {
    matches!(error, crate::Error::LimitExceeded(_) | crate::Error::Cancelled)
}
