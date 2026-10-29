//! Optional timeout-neighbour recovery under one overall supervisor deadline.

use super::{ProcessLimits, invoke_until, protocol, stream};
use crate::read::pdf::{Pages, ReadOptions, Status, bounded};
use crate::read::{Budget, Limits};
use crate::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

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
    let budget = Budget::new(limits, data.len())?;
    budget.check()?;
    let wall_time = limits.timeout.unwrap_or(process.wall_time).min(process.wall_time);
    let deadline = Instant::now()
        .checked_add(wall_time)
        .ok_or(Error::InvalidPdfOption("worker wall time is too large"))?;
    let attempt = process_with_remaining(process, deadline, attempt_timeout)?;
    let attempt_limits = limits.clone().timeout(Some(attempt.wall_time));
    let output = invoke_until(
        worker,
        data,
        protocol::Operation::Pages(options.clone()),
        &attempt_limits,
        &attempt,
        Some(deadline),
    )?;
    let (mut result, first_missing) = stream::decode_recoverable(output, options)?;
    let Some(first_missing) = first_missing else { return Ok(result) };
    let mut remaining =
        options.max_output_bytes - result.pages.iter().map(|page| page.text.len()).sum::<usize>();
    let count = result.pages.len();
    for index in (first_missing + 1..count).chain(std::iter::once(first_missing)) {
        if index >= count || remaining == 0 {
            break;
        }
        // Cancellation is never converted into a per-page failure.
        match budget.check() {
            Ok(()) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => break,
        }
        let Ok(attempt) = process_with_remaining(process, deadline, attempt_timeout) else {
            break;
        };
        let number = result.pages[index].number;
        let selection = ReadOptions {
            first_page: number,
            last_page: number,
            max_pages: 1,
            max_output_bytes: remaining,
            ..options.clone()
        };
        let attempt_limits = limits.clone().timeout(Some(attempt.wall_time));
        let recovered = invoke_until(
            worker,
            data,
            protocol::Operation::Pages(selection.clone()),
            &attempt_limits,
            &attempt,
            Some(deadline),
        )
        .and_then(|output| stream::decode(output, &selection));
        match recovered {
            Ok(mut single) => {
                if single.info != result.info || single.pages.len() != 1 {
                    return Err(Error::malformed("PDF worker", "inconsistent recovery metadata"));
                }
                let page = single.pages.remove(0);
                remaining -= page.text.len();
                result.pages[index] = page;
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error @ Error::LimitExceeded("PDF worker output")) => return Err(error),
            Err(error @ Error::LimitExceeded(_)) => {
                // A retry can expire while spooling, before metadata exists.
                // Keep the prefix even then; no attempt renews the outer clock.
                if Instant::now() >= deadline {
                    break;
                }
                let status =
                    if matches!(error, Error::LimitExceeded("time" | "render worker wall time")) {
                        Status::Skipped
                    } else {
                        Status::Failed
                    };
                result.pages[index] = bounded::unread(number, status, error.to_string());
            }
            // Valid worker failures with metadata already decode to page
            // records. Infrastructure/protocol errors are not page damage.
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

fn process_with_remaining(
    process: &ProcessLimits,
    deadline: Instant,
    attempt_timeout: Duration,
) -> Result<ProcessLimits> {
    let remaining = deadline.saturating_duration_since(Instant::now()).min(attempt_timeout);
    if remaining.is_zero() {
        return Err(Error::LimitExceeded("time"));
    }
    Ok(ProcessLimits { memory_bytes: process.memory_bytes, wall_time: remaining })
}
