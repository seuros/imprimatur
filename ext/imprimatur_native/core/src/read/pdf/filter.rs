//! Stream filters (ISO 32000-1, 7.4): what text and fonts are compressed
//! with. Image codecs are not decoded; nothing here reads images.

#[cfg(test)]
mod tests;

use super::object::{Dict, Object, malformed};
use crate::read::Budget;
use crate::{Error, Result};

/// Applies `filters` in order, each with its parameters.
pub fn decode(
    data: &[u8],
    filters: &[(&[u8], Option<&Dict>)],
    budget: &Budget<'_>,
) -> Result<Vec<u8>> {
    let mut current: Option<Vec<u8>> = None;
    for &(name, params) in filters {
        let input = current.as_deref().unwrap_or(data);
        let output = match name {
            b"FlateDecode" | b"Fl" => predict(inflate(input, budget)?, params)?,
            b"LZWDecode" | b"LZW" => {
                let early = params
                    .and_then(|params| params.get(b"EarlyChange"))
                    .and_then(Object::as_integer)
                    != Some(0);
                predict(lzw(input, early, budget)?, params)?
            }
            b"ASCII85Decode" | b"A85" => ascii85(input),
            b"ASCIIHexDecode" | b"AHx" => super::lexer::hex_bytes(
                input.split(|&byte| byte == b'>').next().unwrap_or_default(),
            ),
            b"RunLengthDecode" | b"RL" => run_length(input),
            b"Crypt" => continue,
            _ => {
                return Err(Error::UnsupportedDocument(format!(
                    "the {} stream filter",
                    String::from_utf8_lossy(name)
                )));
            }
        };
        budget.inflate(output.len())?;
        budget.check()?;
        current = Some(output);
    }
    Ok(current.unwrap_or_else(|| data.to_vec()))
}

/// zlib, or raw deflate when the header is missing. A truncated or
/// corrupt stream yields what inflated before the damage, as viewers show.
fn inflate(data: &[u8], budget: &Budget<'_>) -> Result<Vec<u8>> {
    let zlib = data.len() > 2
        && data[0] & 0x0F == 8
        && (u16::from(data[0]) << 8 | u16::from(data[1])).is_multiple_of(31);
    let expected = data.len().saturating_mul(4);
    let (output, complete) =
        crate::read::inflate::inflate(data, zlib, expected, budget.inflatable())?;
    if complete || !output.is_empty() {
        return Ok(output);
    }
    if zlib {
        // A header that only looked like zlib's: try the bytes as raw deflate.
        let (output, _) =
            crate::read::inflate::inflate(data, false, expected, budget.inflatable())?;
        if !output.is_empty() {
            return Ok(output);
        }
    }
    Err(malformed("a stream does not inflate"))
}

/// Undoes a PNG (10 and up) or TIFF (2) predictor.
fn predict(data: Vec<u8>, params: Option<&Dict>) -> Result<Vec<u8>> {
    let Some(params) = params else { return Ok(data) };
    let number =
        |key: &[u8], default: i64| params.get(key).and_then(Object::as_integer).unwrap_or(default);
    let predictor = number(b"Predictor", 1);
    if predictor < 2 {
        return Ok(data);
    }
    let (colors, bits, columns) =
        (number(b"Colors", 1), number(b"BitsPerComponent", 8), number(b"Columns", 1));
    let invalid = || malformed("bad predictor parameters");
    let bits_per_pixel =
        usize::try_from(colors.checked_mul(bits).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let columns = usize::try_from(columns).map_err(|_| invalid())?;
    let pixel = bits_per_pixel.div_ceil(8).max(1);
    let row = (bits_per_pixel.checked_mul(columns).ok_or_else(invalid)?).div_ceil(8);
    if row == 0 {
        return Err(invalid());
    }
    if predictor == 2 {
        let mut data = data;
        if bits == 8 {
            for line in data.chunks_mut(row) {
                for at in pixel..line.len() {
                    line[at] = line[at].wrapping_add(line[at - pixel]);
                }
            }
        }
        return Ok(data);
    }
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0_u8; row];
    for chunk in data.chunks(row + 1) {
        let (&kind, line) = chunk.split_first().ok_or_else(invalid)?;
        let mut current = line.to_vec();
        current.resize(row, 0);
        for at in 0..row {
            let left = if at >= pixel { current[at - pixel] } else { 0 };
            let up = previous[at];
            let up_left = if at >= pixel { previous[at - pixel] } else { 0 };
            current[at] = current[at].wrapping_add(match kind {
                1 => left,
                2 => up,
                3 => left.midpoint(up),
                4 => paeth(left, up, up_left),
                _ => 0,
            });
        }
        out.extend_from_slice(&current[..line.len().min(row)]);
        previous = current;
    }
    Ok(out)
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(up) - i16::from(up_left);
    let (a, b, c) = (
        (estimate - i16::from(left)).abs(),
        (estimate - i16::from(up)).abs(),
        (estimate - i16::from(up_left)).abs(),
    );
    if a <= b && a <= c {
        left
    } else if b <= c {
        up
    } else {
        up_left
    }
}

/// LZW with 9 to 12 bit codes; `early` is the usual `EarlyChange 1`.
fn lzw(data: &[u8], early: bool, budget: &Budget<'_>) -> Result<Vec<u8>> {
    const CLEAR: u16 = 256;
    const END: u16 = 257;
    // Each code: the code it extends, its last byte, its length.
    let mut prefix = vec![0_u16; 4096];
    let mut last = vec![0_u8; 4096];
    let mut length = vec![0_u16; 4096];
    for (code, byte) in (0..=255_u8).enumerate() {
        last[code] = byte;
        length[code] = 1;
    }
    let (mut next, mut width, mut previous): (u16, u32, Option<u16>) = (258, 9, None);
    let (mut buffer, mut buffered) = (0_u32, 0_u32);
    let mut out = Vec::with_capacity(data.len() * 3);
    let limit = budget.inflatable();
    for &byte in data {
        buffer = buffer << 8 | u32::from(byte);
        buffered += 8;
        while buffered >= width {
            buffered -= width;
            let code = u16::try_from((buffer >> buffered) & ((1 << width) - 1)).unwrap_or(END);
            if code == CLEAR {
                (next, width, previous) = (258, 9, None);
                continue;
            }
            if code == END {
                return Ok(out);
            }
            let known = code < next;
            let source =
                if known { code } else { previous.ok_or_else(|| malformed("bad LZW code"))? };
            let start = out.len();
            let size = usize::from(length[usize::from(source)]);
            out.resize(start + size, 0);
            let mut walk = source;
            for at in (start..start + size).rev() {
                out[at] = last[usize::from(walk)];
                walk = prefix[usize::from(walk)];
            }
            let first = out[start];
            if !known {
                out.push(first);
            }
            if let Some(previous) = previous
                && next < 4096
            {
                prefix[usize::from(next)] = previous;
                last[usize::from(next)] = first;
                length[usize::from(next)] = length[usize::from(previous)].saturating_add(1);
                next += 1;
            }
            previous = Some(code);
            let threshold = u32::from(next) + u32::from(early);
            width = match threshold {
                0..=511 => 9,
                512..=1023 => 10,
                1024..=2047 => 11,
                _ => 12,
            };
            if out.len() > limit {
                return Err(Error::LimitExceeded("decompressed size"));
            }
        }
    }
    Ok(out)
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5 + 4);
    let (mut group, mut count) = (0_u32, 0);
    for &byte in data {
        match byte {
            b'~' => break,
            b'z' if count == 0 => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group = group.wrapping_mul(85).wrapping_add(u32::from(byte - b'!'));
                count += 1;
                if count == 5 {
                    out.extend_from_slice(&group.to_be_bytes());
                    (group, count) = (0, 0);
                }
            }
            _ => {}
        }
    }
    if count > 1 {
        for _ in count..5 {
            group = group.wrapping_mul(85).wrapping_add(84);
        }
        out.extend_from_slice(&group.to_be_bytes()[..count - 1]);
    }
    out
}

fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut at = 0;
    while let Some(&length) = data.get(at) {
        match length {
            128 => break,
            0..=127 => {
                let run = usize::from(length) + 1;
                out.extend_from_slice(&data[at + 1..(at + 1 + run).min(data.len())]);
                at += 1 + run;
            }
            _ => {
                if let Some(&byte) = data.get(at + 1) {
                    out.resize(out.len() + 257 - usize::from(length), byte);
                }
                at += 2;
            }
        }
    }
    out
}
