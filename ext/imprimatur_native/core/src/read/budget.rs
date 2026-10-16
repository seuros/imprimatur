//! The limits a read runs under, and the running tally against them.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{Error, Result};

const MIB: u64 = 1024 * 1024;
/// Time and cancellation are checked once per this many ticks.
const TICKS_PER_CHECK: u32 = 4096;

/// What a read may consume. Documents are untrusted input: a zip bomb, a
/// PDF whose objects refer to each other forever, or a part nested a
/// million deep must fail fast, not take the process down.
///
/// ```
/// use std::time::Duration;
/// use imprimatur::read::Limits;
///
/// let limits = Limits::default().max_inflated(64 * 1024 * 1024).timeout(Some(Duration::from_secs(5)));
/// ```
#[derive(Debug, Clone)]
pub struct Limits {
    max_input: usize,
    max_inflated: u64,
    max_depth: usize,
    timeout: Option<Duration>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input: 512 * 1024 * 1024,
            max_inflated: 1024 * MIB,
            max_depth: 256,
            timeout: Some(Duration::from_mins(1)),
            cancel: None,
        }
    }
}

impl Limits {
    /// The largest document accepted, in bytes; 512 MiB by default.
    #[must_use]
    pub const fn max_input(mut self, bytes: usize) -> Self {
        self.max_input = bytes;
        self
    }

    /// The most bytes decompression may produce in total, across zip
    /// entries and PDF streams; 1 GiB by default.
    #[must_use]
    pub const fn max_inflated(mut self, bytes: u64) -> Self {
        self.max_inflated = bytes;
        self
    }

    /// How deep XML elements, PDF objects and nested PDF forms may go; 256
    /// by default.
    #[must_use]
    pub const fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Wall-clock time a read may take, one minute by default; `None` for none.
    #[must_use]
    pub const fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// A flag another thread sets to stop the read with [`Error::Cancelled`].
    #[must_use]
    pub fn cancel_on(mut self, flag: Arc<AtomicBool>) -> Self {
        self.cancel = Some(flag);
        self
    }
}

/// One read's consumption. Single-threaded: a read runs on one thread.
pub struct Budget<'a> {
    limits: &'a Limits,
    deadline: Option<Instant>,
    inflated: Cell<u64>,
    ticks: Cell<u32>,
}

impl<'a> Budget<'a> {
    pub fn new(limits: &'a Limits, input: usize) -> Result<Self> {
        (input <= limits.max_input).ok_or(Error::LimitExceeded("input size"))?;
        let deadline = limits.timeout.map(|timeout| Instant::now() + timeout);
        Ok(Self { limits, deadline, inflated: Cell::new(0), ticks: Cell::new(0) })
    }

    pub const fn max_depth(&self) -> usize {
        self.limits.max_depth
    }

    /// Decompressed bytes still allowed.
    pub fn inflatable(&self) -> usize {
        let left = self.limits.max_inflated.saturating_sub(self.inflated.get());
        usize::try_from(left).unwrap_or(usize::MAX)
    }

    /// Records `bytes` more of decompressed output.
    pub fn inflate(&self, bytes: usize) -> Result<()> {
        let total = self.inflated.get().saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        (total <= self.limits.max_inflated).ok_or(Error::LimitExceeded("decompressed size"))?;
        self.inflated.set(total);
        Ok(())
    }

    /// Counts one unit of work; every few thousand, checks the clock and
    /// the cancel flag.
    pub fn tick(&self) -> Result<()> {
        let ticks = self.ticks.get().wrapping_add(1);
        self.ticks.set(ticks);
        if ticks.is_multiple_of(TICKS_PER_CHECK) { self.check() } else { Ok(()) }
    }

    /// Checks the clock and the cancel flag now.
    pub fn check(&self) -> Result<()> {
        if self.limits.cancel.as_deref().is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        if self.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Error::LimitExceeded("time"));
        }
        Ok(())
    }

    /// Fails beyond the depth limit.
    pub fn nest(&self, depth: usize) -> Result<()> {
        (depth <= self.limits.max_depth).ok_or(Error::LimitExceeded("nesting depth"))
    }
}
