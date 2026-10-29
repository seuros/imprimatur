//! PDF-specific entry points; all document work stays in the Rust core.

use imprimatur::read::{Limits, pdf};
use magnus::{Error, RString, Ruby};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

use crate::read::without_gvl;
use crate::{argument_error, domain_error};

type Keywords = (Option<usize>, Option<u64>, Option<f64>, bool);

pub fn info(
    ruby: &Ruby,
    document: RString,
    keywords: Keywords,
    worker: String,
    memory_bytes: u64,
) -> Result<RString, Error> {
    let worker = PathBuf::from(worker);
    let process = process_limits(memory_bytes);
    let value = without_gvl(ruby, document, keywords, |data, limits| {
        let info = pdf::isolation::info_isolated(&worker, data, limits, &process)?;
        Ok(json!({ "page_count": info.page_count, "encrypted": info.encrypted }).to_string())
    })?;
    Ok(ruby.str_new(&value))
}

pub fn pages(
    ruby: &Ruby,
    document: RString,
    keywords: Keywords,
    options: (u32, u32, usize, usize, usize, Option<f64>),
    worker: String,
    memory_bytes: u64,
) -> Result<RString, Error> {
    let (first_page, last_page, max_pages, max_page_bytes, max_output_bytes, batch_timeout) =
        options;
    let options =
        pdf::ReadOptions { first_page, last_page, max_pages, max_page_bytes, max_output_bytes };
    let worker = PathBuf::from(worker);
    let process = process_limits(memory_bytes);
    let Some(seconds) = batch_timeout else {
        let result = without_gvl(ruby, document, keywords, |data, limits| {
            pdf::isolation::pages_isolated(&worker, data, &options, limits, &process)
        })?;
        return Ok(ruby.str_new(&pages_json(result)));
    };
    let attempt = Duration::try_from_secs_f64(seconds)
        .map_err(|_| argument_error(ruby, "batch timeout must be finite and positive"))?;
    let mut recovery = pdf::isolation::Recovery::new(&options, attempt)
        .map_err(|error| domain_error(ruby, &error))?;
    loop {
        let step = without_gvl(ruby, document, keywords, |data, limits| {
            let process = pdf::isolation::ProcessLimits {
                wall_time: limits.timeout_duration().unwrap_or(process.wall_time),
                ..process.clone()
            };
            recovery.step(&worker, data, limits, &process)
        })?;
        if let Some(result) = step {
            return Ok(ruby.str_new(&pages_json(result)));
        }
    }
}

fn pages_json(result: pdf::Pages) -> String {
    let pages: Vec<_> = result
        .pages
        .into_iter()
        .map(|page| {
            json!({
                "page": page.number,
                "status": page.status.as_str(),
                "text": page.text,
                "images": page.images,
                "truncated": page.truncated,
                "reason": page.reason,
            })
        })
        .collect();
    json!({
        "page_count": result.info.page_count,
        "encrypted": result.info.encrypted,
        "pages": pages,
    })
    .to_string()
}

pub fn render(
    ruby: &Ruby,
    document: RString,
    keywords: Keywords,
    options: (u32, u16, u64, usize, bool),
    worker: String,
    memory_bytes: u64,
) -> Result<RString, Error> {
    let (page, max_side, max_pixels, max_output_bytes, grayscale) = options;
    let worker = PathBuf::from(worker);
    let options = pdf::RenderOptions { max_side, max_pixels, max_output_bytes, grayscale };
    let process = process_limits(memory_bytes);
    let data = without_gvl(ruby, document, keywords, |data: &[u8], limits: &Limits| {
        pdf::isolation::render_page_isolated(&worker, data, page, &options, limits, &process)
            .map(|image| image.data)
    })?;
    Ok(ruby.str_from_slice(&data))
}

fn process_limits(memory_bytes: u64) -> pdf::isolation::ProcessLimits {
    pdf::isolation::ProcessLimits { memory_bytes, ..pdf::isolation::ProcessLimits::default() }
}
