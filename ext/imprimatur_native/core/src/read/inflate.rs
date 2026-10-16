//! Deflate, for zip entries and PDF streams alike: into one buffer sized
//! up front (exactly, when the size is known), grown only past a guess,
//! never past the budget. What a damaged stream yields before the damage is
//! kept; the caller decides whether that is enough.

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

use crate::{Error, Result};

/// Inflates `data`, zlib-wrapped or raw, starting with room for `expected`
/// bytes and growing to `limit` at most. Returns the output and whether the
/// stream ended properly. Checksums are not verified: PDF writers get them
/// wrong, and zip entries have their CRC checked by the caller.
pub fn inflate(data: &[u8], zlib: bool, expected: usize, limit: usize) -> Result<(Vec<u8>, bool)> {
    let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
        | inflate_flags::TINFL_FLAG_IGNORE_ADLER32
        | if zlib { inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER } else { 0 };
    let mut decompressor = Box::<DecompressorOxide>::default();
    let mut out = vec![0_u8; expected.clamp(1, limit.max(1))];
    let (mut read, mut written) = (0, 0);
    loop {
        let (status, consumed, produced) =
            decompress(&mut decompressor, &data[read..], &mut out, written, flags);
        read += consumed;
        written += produced;
        match status {
            TINFLStatus::Done => {
                out.truncate(written);
                return Ok((out, true));
            }
            TINFLStatus::HasMoreOutput => {
                if out.len() >= limit {
                    return Err(Error::LimitExceeded("decompressed size"));
                }
                out.resize(out.len().saturating_mul(2).min(limit), 0);
            }
            _ => {
                out.truncate(written);
                return Ok((out, false));
            }
        }
    }
}
