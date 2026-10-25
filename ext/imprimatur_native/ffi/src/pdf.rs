//! PDF-specific entry points; all document work stays in the Rust core.

use imprimatur::read::{Limits, pdf};
use magnus::{Error, RString, Ruby};
use serde_json::json;
use std::path::PathBuf;

use crate::read::without_gvl;

type Keywords = (Option<usize>, Option<u64>, Option<f64>);

pub fn info(ruby: &Ruby, document: RString, keywords: Keywords) -> Result<RString, Error> {
    let value = without_gvl(ruby, document, keywords, |data, limits| {
        let info = pdf::info(data, limits)?;
        Ok(json!({ "page_count": info.page_count, "encrypted": info.encrypted }).to_string())
    })?;
    Ok(ruby.str_new(&value))
}

pub fn pages(
    ruby: &Ruby,
    document: RString,
    keywords: Keywords,
    options: (u32, u32, usize, usize, usize),
) -> Result<RString, Error> {
    let (first_page, last_page, max_pages, max_page_bytes, max_output_bytes) = options;
    let options =
        pdf::ReadOptions { first_page, last_page, max_pages, max_page_bytes, max_output_bytes };
    let value = without_gvl(ruby, document, keywords, |data, limits| {
        let result = pdf::pages(data, &options, limits)?;
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
        Ok(json!({
            "page_count": result.info.page_count,
            "encrypted": result.info.encrypted,
            "pages": pages,
        })
        .to_string())
    })?;
    Ok(ruby.str_new(&value))
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
    let process =
        pdf::isolation::ProcessLimits { memory_bytes, ..pdf::isolation::ProcessLimits::default() };
    let data = without_gvl(ruby, document, keywords, |data: &[u8], limits: &Limits| {
        pdf::isolation::render_page_isolated(&worker, data, page, &options, limits, &process)
            .map(|image| image.data)
    })?;
    Ok(ruby.str_from_slice(&data))
}
