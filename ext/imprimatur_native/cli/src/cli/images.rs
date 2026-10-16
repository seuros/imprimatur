use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use imprimatur::read::{Image, ImageLocation, images, json};
use usage_rs::{Args, RunWith};

use super::limits::LimitFlags;
use crate::app::{self, App, Out, Status};

/// Extract documents' images into a directory
///
/// A PDF's images, once each, from the first page that draws them: JPEG
/// and JPEG 2000 as they are stored, fax-compressed scans as TIFF, raw
/// pixels as PNG (CMYK converted to RGB, soft masks as alpha). A scanned
/// PDF gives its page images, ready for OCR. The media of a Word or Excel
/// document are copied, with the sheet that shows each one.
///
/// Files are named after their document: `NAME-p3-1.png` is the first image
/// of page 3; `NAME-image1.png` is a medium of the same name. The paths
/// written are printed, one per line; with `--json`, each image is a JSON
/// object with its format, size and position on the page.
#[derive(Args, Debug)]
#[usage(
    effect = "write",
    args_override_self = false,
    after_long_help = super::EXIT_STATUS,
    example("imprimatur images -o figures/ report.pdf", help = "Every image of a PDF"),
    example("imprimatur images -o pages/ --min-size 600 scan.pdf", help = "Page scans, for OCR"),
    example("imprimatur --json images -o media/ deck.docx", help = "With sizes and positions")
)]
pub struct Images {
    /// Documents to read; `-` reads standard input
    #[usage(arg, required, value_name = "FILE", value_hint = usage_rs::ValueHint::FilePath)]
    pub files: Vec<PathBuf>,

    /// Directory to write the images into (created if missing)
    #[usage(short = 'o', long, value_name = "DIR", value_hint = usage_rs::ValueHint::DirPath)]
    pub output: PathBuf,

    /// Skip images narrower or shorter than this, in pixels
    #[usage(long, default = "0", value_name = "PIXELS")]
    pub min_size: u32,

    #[usage(flatten)]
    pub limits: LimitFlags,
}

impl RunWith<&App> for Images {
    type Output = ExitCode;

    fn run_with(self, app: &App) -> ExitCode {
        let limits = match self.limits.checked() {
            Ok(limits) => limits,
            Err(status) => return status,
        };
        if let Err(status) = app::create_dir(&self.output) {
            return status;
        }
        let mut status = Status::default();
        let mut written = HashSet::new();
        let mut out = Out::new();
        for file in &self.files {
            let mut open = true;
            // The page of the last image, and how many it had so far.
            let mut last = (0, 0);
            let result = app::read(file).and_then(|document| {
                images(&document, &limits, |image| {
                    if image
                        .width
                        .zip(image.height)
                        .is_some_and(|(width, height)| width.min(height) < self.min_size)
                    {
                        return Ok(());
                    }
                    let target = self.target(file, &image, &mut last, &mut written);
                    fs::write(&target, &image.data)?;
                    open &= if app.json {
                        out.json(&describe(file, &target, &image))
                    } else {
                        out.write(&format!("{}\n", target.display()))
                    };
                    // A closed pipe ends the run.
                    if open { Ok(()) } else { Err(imprimatur::Error::Cancelled) }
                })
            });
            if !open {
                break;
            }
            if let Err(error) = result {
                let (failure, still_open) = app.report_failure(&mut out, file, &error);
                status.record(failure);
                if !still_open {
                    break;
                }
            }
        }
        status.exit_code()
    }
}

impl Images {
    /// `DIR/NAME-p3-1.png` for a page's images, `DIR/NAME-image1.png` for a
    /// package's; `-2`, `-3`... when a name is taken.
    fn target(
        &self,
        file: &Path,
        image: &Image,
        last: &mut (u32, usize),
        written: &mut HashSet<PathBuf>,
    ) -> PathBuf {
        let stem = app::stem(file);
        let base = match &image.location {
            ImageLocation::Page { number, .. } => {
                // Images come page by page.
                *last = if last.0 == *number { (*number, last.1 + 1) } else { (*number, 1) };
                format!("{stem}-p{number}-{}", last.1)
            }
            ImageLocation::Part { name, .. } => {
                let part = Path::new(name)
                    .file_stem()
                    .map_or_else(|| "image".into(), |part| part.to_string_lossy());
                format!("{stem}-{part}")
            }
            _ => format!("{stem}-image"),
        };
        app::unique_path(&self.output, &base, image.format.extension(), written)
    }
}

fn describe(file: &Path, target: &Path, image: &Image) -> serde_json::Value {
    let mut object = json::image(image);
    object.insert("file".into(), file.to_string_lossy().into());
    object.insert("image".into(), target.to_string_lossy().into());
    serde_json::Value::Object(object)
}
