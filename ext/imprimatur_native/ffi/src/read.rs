//! `Imprimatur::Native.markdown`, `.content` and `.images`: reading runs
//! without the GVL, so other Ruby threads (and the fibers of a reactor)
//! keep running meanwhile, and a `Thread#raise`, `Timeout` or Ctrl-C
//! cancels it.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use imprimatur::read::{Limits, content, images, json, markdown};
use magnus::{Error, IntoValue, RArray, RString, Ruby};
use serde_json::Value as Json;

use crate::{argument_error, domain_error};

/// Reads interrupted without anything to raise are retried this many times.
const MAX_ATTEMPTS: usize = 64;

/// A read of a document's bytes within limits, run without the GVL.
type Job<T> = fn(&[u8], &Limits) -> imprimatur::Result<T>;

/// What crosses into the thread without the GVL, and comes back.
struct Read<T> {
    document: Vec<u8>,
    limits: Limits,
    job: Job<T>,
    result: Option<imprimatur::Result<T>>,
}

/// Called by `Imprimatur.markdown`, which owns the keyword interface.
/// `timeout` is in seconds; `nil` limits keep their defaults.
pub fn markdown_native(
    ruby: &Ruby,
    document: RString,
    max_input: Option<usize>,
    max_inflated: Option<u64>,
    timeout: Option<f64>,
) -> Result<RString, Error> {
    let text = without_gvl(ruby, document, (max_input, max_inflated, timeout), markdown)?;
    Ok(ruby.str_new(&text))
}

/// Called by `Imprimatur.content`: the structure as JSON, the schema the
/// command line prints, serialized before the GVL is taken back.
pub fn content_native(
    ruby: &Ruby,
    document: RString,
    max_input: Option<usize>,
    max_inflated: Option<u64>,
    timeout: Option<f64>,
) -> Result<RString, Error> {
    let text = without_gvl(ruby, document, (max_input, max_inflated, timeout), |bytes, limits| {
        content(bytes, limits).map(|content| Json::Object(json::content(&content)).to_string())
    })?;
    Ok(ruby.str_new(&text))
}

/// Called by `Imprimatur.images`: `[data, description]` pairs, the data
/// binary and the description JSON.
pub fn images_native(
    ruby: &Ruby,
    document: RString,
    max_input: Option<usize>,
    max_inflated: Option<u64>,
    timeout: Option<f64>,
) -> Result<RArray, Error> {
    let found =
        without_gvl(ruby, document, (max_input, max_inflated, timeout), |bytes, limits| {
            let mut found = Vec::new();
            images(bytes, limits, |image| {
                let description = Json::Object(json::image(&image)).to_string();
                found.push((image.data, description));
                Ok(())
            })?;
            Ok(found)
        })?;
    let pairs = ruby.ary_new_capa(found.len());
    for (data, description) in found {
        let pair = [
            ruby.str_from_slice(&data).into_value_with(ruby),
            ruby.str_new(&description).into_value_with(ruby),
        ];
        pairs.push(ruby.ary_new_from_values(&pair))?;
    }
    Ok(pairs)
}

/// The limits `Imprimatur.*`'s keywords set, stopped by `cancel`.
fn limits(
    ruby: &Ruby,
    cancel: &Arc<AtomicBool>,
    (max_input, max_inflated, timeout): (Option<usize>, Option<u64>, Option<f64>),
) -> Result<Limits, Error> {
    let mut limits = Limits::default().cancel_on(Arc::clone(cancel));
    if let Some(bytes) = max_input {
        limits = limits.max_input(bytes);
    }
    if let Some(bytes) = max_inflated {
        limits = limits.max_inflated(bytes);
    }
    if let Some(seconds) = timeout {
        let timeout = Duration::try_from_secs_f64(seconds)
            .map_err(|_| argument_error(ruby, "timeout must be a positive number of seconds"))?;
        limits = limits.timeout(Some(timeout));
    }
    Ok(limits)
}

/// Runs `job` on the document's bytes on this thread, the GVL released.
fn without_gvl<T>(
    ruby: &Ruby,
    document: RString,
    keywords: (Option<usize>, Option<u64>, Option<f64>),
    job: Job<T>,
) -> Result<T, Error> {
    // SAFETY: the bytes are copied out at once, while the GVL is held and
    // before any Ruby code can run and move or free the string.
    #[allow(unsafe_code)]
    let document = unsafe { document.as_slice() }.to_vec();
    let cancel = Arc::new(AtomicBool::new(false));
    let limits = limits(ruby, &cancel, keywords)?;
    let mut read = Read { document, limits, job, result: None };
    let read_pointer: *mut Read<T> = &raw mut read;
    let flag_pointer = Arc::as_ptr(&cancel).cast_mut();
    for _ in 0..MAX_ATTEMPTS {
        // SAFETY: `read` and `cancel` outlive the call, which returns only
        // once `run` has finished (or never started); `run` touches no Ruby
        // object, and `stop` only an atomic. The `2` variant raises no
        // pending interrupt across these Rust frames.
        #[allow(unsafe_code)]
        unsafe {
            rb_sys::rb_thread_call_without_gvl2(
                Some(run::<T>),
                read_pointer.cast(),
                Some(stop),
                flag_pointer.cast(),
            );
        }
        match read.result.take() {
            Some(Ok(value)) => return Ok(value),
            // Cut short by Ruby: raise what Ruby has to raise (Thread#raise,
            // Timeout, Ctrl-C) through the protected check; with nothing to
            // raise, the wakeup was spurious, and the read starts again.
            Some(Err(imprimatur::Error::Cancelled)) if cancel.swap(false, Ordering::Relaxed) => {
                ruby.thread_check_ints()?;
            }
            Some(Err(error)) => return Err(domain_error(ruby, &error)),
            // An interrupt was pending, so the read never started (thread
            // switching sets one too): let Ruby handle it, then start.
            None => ruby.thread_check_ints()?,
        }
    }
    Err(domain_error(ruby, &imprimatur::Error::Cancelled))
}

/// The read itself, on the thread without the GVL. A panic would abort
/// the process at this C boundary; it becomes an error instead.
#[allow(unsafe_code)]
unsafe extern "C" fn run<T>(data: *mut c_void) -> *mut c_void {
    // SAFETY: `data` is the `Read<T>` without_gvl passed, alive and not
    // otherwise borrowed until this returns.
    let read = unsafe { &mut *data.cast::<Read<T>>() };
    let result = catch_unwind(AssertUnwindSafe(|| (read.job)(&read.document, &read.limits)));
    read.result = Some(result.unwrap_or_else(|_| {
        Err(imprimatur::Error::Malformed {
            format: "document",
            reason: "the reader failed on it".to_owned(),
        })
    }));
    std::ptr::null_mut()
}

/// Ruby's unblocking function: asks the read to stop.
#[allow(unsafe_code)]
unsafe extern "C" fn stop(data: *mut c_void) {
    // SAFETY: `data` is the cancel flag, kept alive by without_gvl's Arc for
    // the whole blocking region.
    let flag = unsafe { &*data.cast::<AtomicBool>() };
    flag.store(true, Ordering::Relaxed);
}
