#![cfg(all(feature = "isolation", unix))]

use std::assert_matches;
use std::path::Path;

use imprimatur::Error;
use imprimatur::read::Limits;
use imprimatur::read::pdf::RenderOptions;
use imprimatur::read::pdf::isolation::{ProcessLimits, render_page_isolated};

fn pdf() -> Vec<u8> {
    // A native-text page exercises the real reader, renderer and reply protocol.
    b"%PDF-1.4\n\
      1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
      2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
      3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] \
      /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n\
      4 0 obj << /Length 40 >> stream\nBT /F1 12 Tf 20 260 Td (Heading) Tj ET\nendstream endobj\n\
      5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n\
      trailer << /Root 1 0 R >>\n%%EOF"
        .to_vec()
}

#[test]
fn real_worker_renders_and_enforces_limits_without_fallback() {
    let worker = Path::new(env!("CARGO_BIN_EXE_imprimatur-render-worker"));
    let data = pdf();
    let options = RenderOptions { max_side: 300, ..RenderOptions::default() };
    let limits = Limits::default();
    let process = ProcessLimits::default();
    let image = render_page_isolated(worker, &data, 1, &options, &limits, &process).unwrap();
    assert_eq!((image.width, image.height), (Some(200), Some(300)));
    assert!(image.data.starts_with(b"\x89PNG\r\n\x1a\n"));
    let tiny = RenderOptions { max_output_bytes: 1, ..options };
    assert_matches!(
        render_page_isolated(worker, &data, 1, &tiny, &limits, &process),
        Err(Error::LimitExceeded(_))
    );
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let starved = ProcessLimits { memory_bytes: 1, ..process.clone() };
        assert_matches!(
            render_page_isolated(worker, &data, 1, &options, &limits, &starved),
            Err(Error::LimitExceeded(_))
        );
    }
    assert_matches!(
        render_page_isolated(worker, b"not a PDF", 1, &options, &limits, &process),
        Err(Error::UnsupportedDocument(_))
    );
    assert_matches!(
        render_page_isolated(
            Path::new("/missing/imprimatur-render-worker"),
            &data,
            1,
            &options,
            &limits,
            &process
        ),
        Err(Error::InvalidPdfOption(_))
    );
}
