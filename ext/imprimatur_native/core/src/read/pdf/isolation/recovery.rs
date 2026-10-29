//! Optional timeout-neighbour recovery under one overall supervisor deadline.

use std::iter;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{ProcessLimits, invoke, protocol, stream};
use crate::read::Limits;
use crate::read::pdf::{Pages, ReadOptions, Status, bounded};
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Reads with a shorter worker-attempt timeout, recovering unread neighbours.
///
/// `limits.timeout` and `process.wall_time` bound the entire operation, not
/// each attempt. Completed records survive; timeout-unread pages are tried
/// once individually, later neighbours before the first missing page.
/// All workers retain the input, decompression, depth and process-memory caps.
/// Decompression is bounded per worker, as in ordinary page-local recovery;
/// retained text shares `options.max_output_bytes` across all attempts.
///
/// # Errors
/// As [`super::pages_isolated`], or a zero attempt timeout.
pub fn pages_isolated_recovering(
    worker: &Path,
    data: &[u8],
    options: &ReadOptions,
    limits: &Limits,
    process: &ProcessLimits,
    attempt_timeout: Duration,
) -> Result<Pages> {
    if attempt_timeout.is_zero() {
        return Err(Error::InvalidPdfOption("batch timeout must be positive"));
    }
    let deadline = Instant::now()
        .checked_add(process.wall_time_for(limits))
        .ok_or(Error::InvalidPdfOption("worker wall time is too large"))?;
    let attempt = |selection: &ReadOptions| {
        let wall_time = deadline.saturating_duration_since(Instant::now()).min(attempt_timeout);
        if wall_time.is_zero() {
            return Err(Error::LimitExceeded("time"));
        }
        let process = ProcessLimits { wall_time, ..process.clone() };
        let operation = protocol::Operation::Pages(selection.clone());
        stream::decode_recoverable(invoke(worker, data, operation, limits, &process)?, selection)
    };
    let (mut result, first_missing) = attempt(options)?;
    let Some(first_missing) = first_missing else { return Ok(result) };
    let mut remaining =
        options.max_output_bytes - result.pages.iter().map(|page| page.text.len()).sum::<usize>();
    let count = result.pages.len();
    for index in (first_missing + 1..count).chain(iter::once(first_missing)) {
        if index >= count || remaining == 0 {
            break;
        }
        let number = result.pages[index].number;
        let selection = ReadOptions {
            first_page: number,
            last_page: number,
            max_pages: 1,
            max_output_bytes: remaining,
            ..options.clone()
        };
        match attempt(&selection) {
            Ok((mut single, _)) => {
                if single.info != result.info || single.pages.len() != 1 {
                    return Err(Error::malformed("PDF worker", "inconsistent recovery metadata"));
                }
                let page = single.pages.remove(0);
                remaining -= page.text.len();
                result.pages[index] = page;
            }
            Err(error @ Error::LimitExceeded(reason)) if reason != "PDF worker output" => {
                // A retry can expire while spooling, before metadata exists.
                // Keep the prefix even then; no attempt renews the outer clock.
                if Instant::now() >= deadline {
                    break;
                }
                let status = if stream::expired(&error) { Status::Skipped } else { Status::Failed };
                result.pages[index] = bounded::unread(number, status, error.to_string());
            }
            // Valid worker failures with metadata already decode to page
            // records. Infrastructure/protocol errors are not page damage.
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}
