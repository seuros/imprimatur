#![cfg(all(feature = "isolation", unix))]

use std::assert_matches;
use std::path::Path;

use imprimatur::Error;
use imprimatur::read::Limits;
use imprimatur::read::pdf::isolation::{
    ProcessLimits, info_isolated, pages_isolated, render_page_isolated,
};
use imprimatur::read::pdf::{self, ReadOptions, RenderOptions, Status};

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
    assert_eq!(info_isolated(worker, &data, &limits, &process).unwrap().page_count, 1);
    let read = ReadOptions::default();
    assert_eq!(
        pages_isolated(worker, &data, &read, &limits, &process).unwrap().pages[0].text,
        "Heading"
    );
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
            info_isolated(worker, &data, &limits, &starved),
            Err(Error::LimitExceeded(_))
        );
        assert_matches!(
            pages_isolated(worker, &data, &read, &limits, &starved),
            Err(Error::LimitExceeded(_))
        );
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
    assert_matches!(
        info_isolated(Path::new("/missing/worker"), &data, &limits, &process),
        Err(Error::InvalidPdfOption(_))
    );
    assert_matches!(
        pages_isolated(Path::new("/missing/worker"), &data, &read, &limits, &process),
        Err(Error::InvalidPdfOption(_))
    );
}

#[test]
fn a_thrown_decompression_limit_preserves_healthy_neighbors() {
    let mut doc = imprimatur::doc::Document::new();
    doc.paragraph("Healthy first page").unwrap();
    doc.page_break();
    doc.paragraph("x".repeat(2000)).unwrap();
    doc.page_break();
    doc.paragraph("Healthy last page").unwrap();
    let data = doc.to_pdf().unwrap();
    let limits = Limits::default().max_inflated(1024);
    let options = ReadOptions::default();
    // This is a thrown document-budget failure, not a returned corrupt-stream
    // status: the old batch path discarded both healthy neighbors.
    assert_matches!(pdf::pages(&data, &options, &limits), Err(Error::LimitExceeded(_)));
    let result = pages_isolated(
        Path::new(env!("CARGO_BIN_EXE_imprimatur-render-worker")),
        &data,
        &options,
        &limits,
        &ProcessLimits::default(),
    )
    .unwrap();
    assert_eq!(
        result.pages.iter().map(|page| page.status).collect::<Vec<_>>(),
        [Status::Text, Status::Failed, Status::Text]
    );
    assert_eq!(result.pages[0].text, "Healthy first page");
    assert_eq!(result.pages[2].text, "Healthy last page");
    assert!(result.pages[1].reason.as_ref().unwrap().contains("limit"));

    let options = ReadOptions { max_output_bytes: "Healthy first page".len() + 5, ..options };
    let result = pages_isolated(
        Path::new(env!("CARGO_BIN_EXE_imprimatur-render-worker")),
        &data,
        &options,
        &limits,
        &ProcessLimits::default(),
    )
    .unwrap();
    assert_eq!(result.pages[2].text, "Healt");
    assert!(result.pages[2].truncated, "recovery must not renew the aggregate output cap");
}
