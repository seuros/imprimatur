//! Counts the heap allocations reading takes, per document: reading its
//! content, then rendering it as Markdown.
//! `cargo run --release --example allocations -- FILE...`. A measuring
//! stick for keeping the readers lean.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use imprimatur::read::{Limits, content};

static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the counters are atomics.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller upholds `alloc`'s contract, as `System` needs.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `System` with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
        // SAFETY: as `dealloc`, and the caller keeps `size` valid.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations and bytes allocated since `from`.
fn since(from: (usize, usize)) -> (usize, usize) {
    (CALLS.load(Ordering::Relaxed) - from.0, BYTES.load(Ordering::Relaxed) - from.1)
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(document) = std::fs::read(&path) else {
            println!("unreadable\t{path}");
            continue;
        };
        let start = since((0, 0));
        let read = content(&document, &Limits::default());
        let (calls, bytes) = since(start);
        let rendering = since((0, 0));
        let out = read.map_or_else(
            |error| error.to_string(),
            |content| format!("{} bytes out", content.to_markdown().len()),
        );
        let (render, _) = since(rendering);
        println!("{calls}\tallocations\t{render}\trendering\t{}\tKiB\t{out}\t{path}", bytes / 1024);
    }
}
