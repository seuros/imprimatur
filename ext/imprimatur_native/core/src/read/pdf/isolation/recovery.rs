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
    let mut recovery = Recovery::new(options, attempt_timeout)?;
    loop {
        if let Some(pages) = recovery.step(worker, data, limits, process)? {
            return Ok(pages);
        }
    }
}

/// [`pages_isolated_recovering`], one worker attempt per [`Recovery::step`].
///
/// A step that fails keeps the progress before it, and the deadline set by
/// the first step: a caller woken spuriously can step again.
pub struct Recovery {
    options: ReadOptions,
    attempt_timeout: Duration,
    deadline: Option<Instant>,
    result: Option<Pages>,
    pending: Vec<usize>,
    remaining: usize,
}

impl Recovery {
    /// # Errors
    /// A zero attempt timeout.
    pub fn new(options: &ReadOptions, attempt_timeout: Duration) -> Result<Self> {
        if attempt_timeout.is_zero() {
            return Err(Error::InvalidPdfOption("batch timeout must be positive"));
        }
        Ok(Self {
            options: options.clone(),
            attempt_timeout,
            deadline: None,
            result: None,
            pending: Vec::new(),
            remaining: options.max_output_bytes,
        })
    }

    /// Runs the next worker attempt; the pages once recovery is over.
    ///
    /// # Errors
    /// As [`pages_isolated_recovering`].
    pub fn step(
        &mut self,
        worker: &Path,
        data: &[u8],
        limits: &Limits,
        process: &ProcessLimits,
    ) -> Result<Option<Pages>> {
        let deadline = self.deadline(limits, process)?;
        let attempt_timeout = self.attempt_timeout;
        let attempt = |selection: &ReadOptions| {
            let wall_time = deadline.saturating_duration_since(Instant::now()).min(attempt_timeout);
            if wall_time.is_zero() {
                return Err(Error::LimitExceeded("time"));
            }
            let process = ProcessLimits { wall_time, ..process.clone() };
            let operation = protocol::Operation::Pages(selection.clone());
            stream::decode_recoverable(
                invoke(worker, data, operation, limits, &process)?,
                selection,
            )
        };
        let Some(result) = &mut self.result else {
            let (pages, first_missing) = attempt(&self.options)?;
            let Some(first_missing) = first_missing else { return Ok(Some(pages)) };
            let count = pages.pages.len();
            self.pending = (first_missing + 1..count)
                .chain(iter::once(first_missing))
                .filter(|&index| index < count)
                .rev()
                .collect();
            self.remaining -= pages.pages.iter().map(|page| page.text.len()).sum::<usize>();
            self.result = Some(pages);
            return Ok(self.settle());
        };
        let Some(&index) = self.pending.last() else { return Ok(self.result.take()) };
        let number = result.pages[index].number;
        let selection = ReadOptions {
            first_page: number,
            last_page: number,
            max_pages: 1,
            max_output_bytes: self.remaining,
            ..self.options.clone()
        };
        match attempt(&selection) {
            Ok((mut single, _)) => {
                if single.info != result.info || single.pages.len() != 1 {
                    return Err(Error::malformed("PDF worker", "inconsistent recovery metadata"));
                }
                let page = single.pages.remove(0);
                self.remaining -= page.text.len();
                result.pages[index] = page;
            }
            Err(error @ Error::LimitExceeded(reason)) if reason != "PDF worker output" => {
                // A retry can expire while spooling, before metadata exists.
                // Keep the prefix even then; no attempt renews the outer clock.
                if Instant::now() >= deadline {
                    return Ok(self.result.take());
                }
                let status = if stream::expired(&error) { Status::Skipped } else { Status::Failed };
                result.pages[index] = bounded::unread(number, status, error.to_string());
            }
            // Valid worker failures with metadata already decode to page
            // records. Infrastructure/protocol errors are not page damage.
            Err(error) => return Err(error),
        }
        self.pending.pop();
        Ok(self.settle())
    }

    fn deadline(&mut self, limits: &Limits, process: &ProcessLimits) -> Result<Instant> {
        if let Some(deadline) = self.deadline {
            return Ok(deadline);
        }
        let deadline = Instant::now()
            .checked_add(process.wall_time_for(limits))
            .ok_or(Error::InvalidPdfOption("worker wall time is too large"))?;
        Ok(*self.deadline.insert(deadline))
    }

    const fn settle(&mut self) -> Option<Pages> {
        if self.pending.is_empty() || self.remaining == 0 { self.result.take() } else { None }
    }
}
