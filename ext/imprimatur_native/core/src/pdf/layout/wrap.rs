//! Greedy line breaking across runs of encoded text.

#[cfg(test)]
mod tests;

use super::super::font::Face;
use crate::style::Color;

/// How a piece of text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub face: Face,
    pub color: Option<Color>,
    pub underline: bool,
}

/// Text with one look on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub bytes: Vec<u8>,
    pub look: Look,
    pub width: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub fragments: Vec<Fragment>,
    pub width: f64,
}

impl Line {
    /// Appends text, merging it into the last fragment when the look matches.
    fn push(&mut self, bytes: &[u8], look: Look, width: f64) {
        self.width += width;
        if let Some(last) = self.fragments.last_mut()
            && last.look == look
        {
            last.bytes.extend_from_slice(bytes);
            last.width += width;
            return;
        }
        self.fragments.push(Fragment { bytes: bytes.to_vec(), look, width });
    }

    const fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }
}

/// Spaces a tab stands for.
const TAB_SPACES: &[u8] = b"    ";

/// Breaks `runs` into lines no wider than `max_width` at `size` points.
/// Lines break at spaces; `\n` forces a break; a word wider than a line is
/// split between characters. Spaces where a line wraps are dropped.
pub fn wrap(runs: &[(Vec<u8>, Look)], size: f64, max_width: f64) -> Vec<Line> {
    let mut lines = vec![Line::default()];
    // Whitespace seen since the last word, placed only if a word follows on
    // the same line.
    let mut pending: Vec<(Vec<u8>, Look)> = Vec::new();
    for (bytes, look) in runs {
        let face = look.face;
        for token in tokens(bytes) {
            match token {
                Token::Newline => {
                    lines.push(Line::default());
                    pending.clear();
                }
                Token::Space(space) => {
                    let mut expanded = Vec::with_capacity(space.len());
                    for &byte in space {
                        if byte == b'\t' {
                            expanded.extend_from_slice(TAB_SPACES);
                        } else {
                            expanded.push(byte);
                        }
                    }
                    pending.push((expanded, *look));
                }
                Token::Word(word) => {
                    let width = face.width(word, size);
                    let spaces: f64 =
                        pending.iter().map(|(space, look)| look.face.width(space, size)).sum();
                    let line = lines.last_mut().map_or(0.0, |line| line.width);
                    let started = lines.last().is_some_and(|line| !line.is_empty());
                    if started && line + spaces + width > max_width {
                        lines.push(Line::default());
                        pending.clear();
                    }
                    let Some(line) = lines.last_mut() else { continue };
                    for (space, look) in std::mem::take(&mut pending) {
                        let width = look.face.width(&space, size);
                        line.push(&space, look, width);
                    }
                    place_word(&mut lines, word, *look, size, max_width);
                }
            }
        }
    }
    lines
}

/// Places a word on the last line, splitting it across lines when it is
/// wider than a whole line (at least one character per line).
fn place_word(lines: &mut Vec<Line>, word: &[u8], look: Look, size: f64, max_width: f64) {
    let mut rest = word;
    while !rest.is_empty() {
        let Some(line) = lines.last_mut() else { return };
        let available = max_width - line.width;
        let mut fit = 0;
        let mut width = 0.0;
        for &byte in rest {
            let next = look.face.width(&[byte], size);
            if width + next > available && (fit > 0 || !line.is_empty()) {
                break;
            }
            width += next;
            fit += 1;
        }
        if fit == 0 {
            lines.push(Line::default());
            continue;
        }
        line.push(&rest[..fit], look, width);
        rest = &rest[fit..];
        if !rest.is_empty() {
            lines.push(Line::default());
        }
    }
}

enum Token<'a> {
    Word(&'a [u8]),
    Space(&'a [u8]),
    Newline,
}

/// Words, runs of spaces and tabs, and newlines.
fn tokens(bytes: &[u8]) -> impl Iterator<Item = Token<'_>> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        let first = *rest.first()?;
        let kind = |byte: u8| match byte {
            b'\n' => 0,
            b' ' | b'\t' => 1,
            _ => 2,
        };
        let length = if first == b'\n' {
            1
        } else {
            rest.iter().position(|&byte| kind(byte) != kind(first)).unwrap_or(rest.len())
        };
        let (token, tail) = rest.split_at(length);
        rest = tail;
        Some(match kind(first) {
            0 => Token::Newline,
            1 => Token::Space(token),
            _ => Token::Word(token),
        })
    })
}
