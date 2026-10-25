use std::assert_matches;

use super::*;
use crate::test_support::{pdf_of, png_of, stream_object};

fn heading_over_scan(image_side: u32) -> Vec<u8> {
    pdf_of(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 4 0 R \
          /Resources << /Font << /F1 5 0 R >> /XObject << /Scan 6 0 R >> >> >>"
            .to_vec(),
        stream_object(
            "",
            b"BT /F1 12 Tf 20 260 Td (Heading) Tj ET q 100 0 0 100 20 80 cm /Scan Do Q",
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream_object(
            &format!(
                "/Type /XObject /Subtype /Image /Width {image_side} /Height {image_side} \
                      /ColorSpace /DeviceGray /BitsPerComponent 8"
            ),
            &[0; 4],
        ),
    ])
}

#[test]
fn rasterizes_native_text_and_the_scan_in_their_page_positions() {
    let options = RenderOptions { max_side: 300, ..RenderOptions::default() };
    let image = render_page(&heading_over_scan(2), 1, &options, &Limits::default()).unwrap();
    let png = png_of(&image.data);
    assert_eq!((png.width, png.height, png.color), (200, 300, 0));
    assert!(
        png.rows[20 * 200..50 * 200].iter().any(|&pixel| pixel < 128),
        "the native heading must be drawn"
    );
    assert!(
        png.rows[130 * 200 + 40..130 * 200 + 80].iter().all(|&pixel| pixel < 16),
        "the scan must be drawn"
    );
    assert_eq!(png.rows[290 * 200 + 190], 255, "the page background must be opaque white");
}

#[test]
fn refuses_oversized_output_and_expired_requests() {
    let options = RenderOptions { max_pixels: 6, max_output_bytes: 3, ..RenderOptions::default() };
    for size in [(0, 2), (2, 0), (2, 4), (u32::MAX, u32::MAX)] {
        assert_matches!(
            options.image(Vec::new(), 1, size),
            Err(Error::LimitExceeded("rendered pixels")),
            "{size:?}"
        );
    }
    assert_matches!(
        options.image(vec![0; 4], 1, (2, 3)),
        Err(Error::LimitExceeded("rendered bytes"))
    );
    let image = options.image(vec![0; 3], 2, (2, 3)).unwrap();
    assert_eq!((image.width, image.height, image.data.len()), (Some(2), Some(3), 3));
    assert_eq!(image.location, ImageLocation::Page { number: 2, bounds: None });

    let data = heading_over_scan(2);
    let options = RenderOptions { max_pixels: 1, ..RenderOptions::default() };
    assert_matches!(
        render_page(&data, 1, &options, &Limits::default()),
        Err(Error::LimitExceeded("rendered pixels"))
    );
    let options = RenderOptions { max_output_bytes: 1, ..RenderOptions::default() };
    assert_matches!(
        render_page(&data, 1, &options, &Limits::default()),
        Err(Error::LimitExceeded("rendered bytes"))
    );
    let limits = Limits::default().timeout(Some(std::time::Duration::ZERO));
    assert_matches!(
        render_page(&data, 1, &RenderOptions::default(), &limits),
        Err(Error::LimitExceeded("time"))
    );
    assert_matches!(
        render_page(&heading_over_scan(10_000), 1, &RenderOptions::default(), &Limits::default()),
        Err(Error::LimitExceeded("source image pixels"))
    );
}
