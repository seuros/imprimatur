//! Full-page rasterization. Optional, pure Rust, and independent of text extraction.

#[cfg(test)]
mod tests;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use super::bounded;
use super::content::{Drawing, page_images};
use crate::read::image::{Image, ImageFormat, ImageLocation, PngColor, png};
use crate::read::{Budget, Limits};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Longest output side in pixels; 1568 by default.
    pub max_side: u16,
    /// Maximum output surface, before allocating it.
    pub max_pixels: u64,
    pub max_output_bytes: usize,
    pub grayscale: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            max_side: 1568,
            max_pixels: 4_000_000,
            max_output_bytes: 8 * 1024 * 1024,
            grayscale: true,
        }
    }
}

impl RenderOptions {
    pub(super) fn check_pixels(&self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > self.max_pixels {
            return Err(Error::LimitExceeded("rendered pixels"));
        }
        Ok(())
    }

    pub(super) fn image(&self, data: Vec<u8>, number: u32, size: (u32, u32)) -> Result<Image> {
        self.check_pixels(size.0, size.1)?;
        if data.len() > self.max_output_bytes {
            return Err(Error::LimitExceeded("rendered bytes"));
        }
        Ok(Image {
            data,
            format: ImageFormat::Png,
            width: Some(size.0),
            height: Some(size.1),
            location: ImageLocation::Page { number, bounds: None },
        })
    }
}

/// A one-based page, including native text and graphics, as an opaque PNG.
///
/// Output dimensions and encoded bytes are bounded. Hayro has no cancellation
/// hook: time/cancellation are checked before and after rasterization, not
/// during it. Use `isolation::render_page_isolated` for untrusted renders.
/// `max_inflated` bounds our preflight, not Hayro's internal allocations.
///
/// # Errors
/// As [`super::info`], invalid options/page number, an oversized surface or
/// output, and renderer warnings (rather than silently missing content).
pub fn render_page(
    data: &[u8],
    number: u32,
    options: &RenderOptions,
    limits: &Limits,
) -> Result<Image> {
    if number == 0
        || options.max_side == 0
        || options.max_pixels == 0
        || options.max_output_bytes == 0
    {
        return Err(Error::InvalidPdfOption("page number and rendering limits must be positive"));
    }
    let budget = Budget::new(limits, data.len())?;
    let inspected = bounded::info(data, limits)?;
    if number > inspected.page_count {
        return Err(Error::InvalidPdfOption("page number is past the end of the document"));
    }
    budget.check()?;
    preflight(data, number, &budget)?;
    catch_unwind(AssertUnwindSafe(|| render(data, number, options, &budget)))
        .unwrap_or_else(|_| Err(Error::malformed("PDF", "the renderer failed on this page")))
}

fn preflight(data: &[u8], number: u32, budget: &Budget<'_>) -> Result<()> {
    let document = bounded::open(data, budget)?;
    let selected = document.select_pages(number, number, 1)?;
    let (_, page) = selected
        .pages
        .into_iter()
        .next()
        .ok_or(Error::InvalidPdfOption("page number is past the end of the document"))?;
    let mut drawn = Vec::new();
    page_images(&document, &page?, &mut drawn)?;
    let mut pixels = 0_u64;
    for image in drawn {
        budget.check()?;
        let dict = match &image.image {
            Drawing::Object { object, .. } => object.as_dict(),
            Drawing::Inline { dict, .. } => Some(dict),
        }
        .ok_or_else(|| Error::malformed("PDF", "invalid image dictionary"))?;
        let dimension = |full: &[u8], short: &[u8]| -> Result<u64> {
            let value = document.entry(dict, full)?.or(document.entry(dict, short)?);
            let value = value
                .and_then(|value| value.as_integer())
                .and_then(|value| u64::try_from(value).ok());
            value
                .filter(|&value| value != 0)
                .ok_or_else(|| Error::malformed("PDF", "invalid image dimensions"))
        };
        let size = dimension(b"Width", b"W")?
            .checked_mul(dimension(b"Height", b"H")?)
            .ok_or(Error::LimitExceeded("source image pixels"))?;
        pixels = pixels.saturating_add(size);
        if pixels > 40_000_000 {
            return Err(Error::LimitExceeded("source image pixels"));
        }
    }
    Ok(())
}

fn render(data: &[u8], number: u32, options: &RenderOptions, budget: &Budget<'_>) -> Result<Image> {
    let pdf = Pdf::new(data.to_vec()).map_err(|error| match error {
        LoadPdfError::Decryption(_) => Error::PasswordRequired,
        LoadPdfError::Invalid => Error::malformed("PDF", "the renderer cannot open this document"),
    })?;
    budget.check()?;
    let page = pdf.pages().get(number as usize - 1).ok_or_else(|| {
        Error::malformed("PDF", "the renderer's page tree differs from the reader's")
    })?;
    let (width, height) = page.render_dimensions();
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(Error::malformed("PDF", "invalid page dimensions"));
    }
    let scale = f32::from(options.max_side) / width.max(height);
    let pixels = |side: f32| (side * scale).floor().max(1.0).min(f32::from(options.max_side));
    let (target_width, target_height) = (pixels(width), pixels(height));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded by max_side"
    )]
    let (w, h) = (target_width as u32, target_height as u32);
    options.check_pixels(w, h)?;
    let warned = Arc::new(AtomicBool::new(false));
    let warning = Arc::clone(&warned);
    let settings = InterpreterSettings {
        warning_sink: Arc::new(move |_| {
            warning.store(true, Ordering::Relaxed);
        }),
        ..InterpreterSettings::default()
    };
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &settings,
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: target_width / width,
            y_scale: target_height / height,
            bg_color: WHITE,
        },
    );
    budget.check()?;
    if warned.load(Ordering::Relaxed) {
        return Err(Error::UnsupportedDocument(
            "the renderer could not render all page content".into(),
        ));
    }
    let (w, h) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    options.check_pixels(w, h)?;
    let rgba = pixmap.data_as_u8_slice();
    let encoded = if options.grayscale {
        let gray: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| {
                let value = u32::from(pixel[0]) * 299
                    + u32::from(pixel[1]) * 587
                    + u32::from(pixel[2]) * 114;
                u8::try_from(value / 1000).unwrap_or(255)
            })
            .collect();
        png(w, h, PngColor::Gray, 8, None, &gray)
    } else {
        png(w, h, PngColor::Rgba, 8, None, rgba)
    };
    budget.check()?;
    options.image(encoded, number, (w, h))
}
