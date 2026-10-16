//! Prints documents as Markdown: `cargo run --example markdown -- FILE...`.
//! With `--stats`, prints one line per file instead: characters out,
//! milliseconds, or the error.

use std::time::Instant;

use imprimatur::read::{Limits, markdown};

fn main() {
    let mut stats = false;
    for argument in std::env::args().skip(1) {
        if argument == "--stats" {
            stats = true;
            continue;
        }
        let started = Instant::now();
        let result = std::fs::read(&argument)
            .map_err(imprimatur::Error::from)
            .and_then(|bytes| markdown(&bytes, &Limits::default()));
        let elapsed = started.elapsed().as_millis();
        match (result, stats) {
            (Ok(text), true) => {
                let visible = text.chars().filter(|ch| !ch.is_whitespace()).count();
                println!("ok\t{visible}\t{elapsed}ms\t{argument}");
            }
            (Ok(text), false) => print!("{text}"),
            (Err(error), _) => println!("error\t{error}\t{elapsed}ms\t{argument}"),
        }
    }
}
