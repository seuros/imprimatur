use std::assert_matches;
use std::fmt::Write as _;

use crate::Error;
use crate::read::{Bounds, Image, ImageFormat, ImageLocation, Limits, images};
use crate::test_support::{pdf_of, png_of, stream_object, tiff_of};

const IMAGE: &str = "/Type /XObject /Subtype /Image";

/// The entries and samples of an image, then its PNG's color type, bits and
/// rows.
type Case = (&'static str, &'static [u8], u8, u8, &'static [u8]);

/// One page drawing `content`; `others` are objects 5 on, named `/X5`,
/// `/X6`... in its resources.
fn page_of(content: &str, others: &[Vec<u8>]) -> Vec<u8> {
    let mut names = String::new();
    for number in 5..5 + others.len() {
        write!(names, "/X{number} {number} 0 R ").unwrap();
    }
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /XObject << {names}>> >> >>"
        )
        .into_bytes(),
        stream_object("", content.as_bytes()),
    ];
    objects.extend_from_slice(others);
    pdf_of(&objects)
}

fn extract(pdf: &[u8]) -> Vec<Image> {
    let mut found = Vec::new();
    images(pdf, &Limits::default(), |image| {
        found.push(image);
        Ok(())
    })
    .unwrap();
    found
}

/// An image drawn at `/X5`, with an ICC profile of one component as
/// object 6 and a 2 by 1 soft mask as object 7.
fn drawn(dict: &str, samples: &[u8]) -> Vec<Image> {
    extract(&page_of(
        "/X5 Do",
        &[
            stream_object(&format!("{IMAGE} {dict}"), samples),
            stream_object("/N 1", b"profile"),
            stream_object(
                &format!("{IMAGE} /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8"),
                &[128, 255],
            ),
        ],
    ))
}

#[test]
fn writes_raw_pixels_as_png() {
    const CASES: &[Case] = &[
        (
            "/Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0, 200],
            0,
            8,
            &[0, 200],
        ),
        (
            "/Width 2 /Height 1 /ColorSpace /G /BitsPerComponent 8 /Decode [1 0]",
            &[0, 200],
            0,
            8,
            &[255, 55],
        ),
        (
            "/Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 16",
            &[1, 2, 3, 4, 5, 6],
            2,
            16,
            &[1, 2, 3, 4, 5, 6],
        ),
        (
            "/Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 4",
            &[0xF0, 0x80],
            2,
            8,
            &[255, 0, 136],
        ),
        (
            "/Width 1 /Height 1 /ColorSpace /DeviceCMYK /BitsPerComponent 8",
            &[0, 255, 0, 51],
            2,
            8,
            &[204, 0, 204],
        ),
        (
            "/Width 1 /Height 1 /ColorSpace [/Separation /Spot /DeviceCMYK 9 0 R]",
            &[230],
            0,
            8,
            &[25],
        ),
        ("/Width 1 /Height 1 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8", &[7], 0, 8, &[7]),
        ("/Width 8 /Height 1 /ImageMask true", &[0b1010_0000], 0, 1, &[0b1010_0000]),
        (
            "/Width 9 /Height 1 /BitsPerComponent 1 \
             /ColorSpace [/Indexed /DeviceRGB 1 <FF000000FF00>]",
            &[0x80, 0x80],
            3,
            1,
            &[0x80, 0x80],
        ),
        (
            "/Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 7 0 R",
            &[1, 2, 3, 4, 5, 6],
            6,
            8,
            &[1, 2, 3, 128, 4, 5, 6, 255],
        ),
        (
            "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 7 0 R",
            &[9],
            0,
            8,
            &[9],
        ),
    ];
    for &(dict, samples, color, bits, rows) in CASES {
        let found = drawn(dict, samples);
        let [image] = found.as_slice() else { panic!("{dict}: {found:?}") };
        assert_eq!(image.format, ImageFormat::Png, "{dict}");
        let png = png_of(&image.data);
        assert_eq!((png.color, png.bits, png.rows.as_slice()), (color, bits, rows), "{dict}");
        assert_eq!((image.width, image.height), (Some(png.width), Some(png.height)), "{dict}");
    }
    let indexed =
        drawn("/Width 1 /Height 1 /ColorSpace [/Indexed [/ICCBased 6 0 R] 1 <00FF>]", &[0]);
    let palette = png_of(&indexed[0].data).palette;
    assert_eq!(palette.as_deref(), Some(&[0, 0, 0, 255, 255, 255][..]), "a gray palette as RGB");
}

#[test]
fn skips_what_it_cannot_write() {
    const CASES: &[&str] = &[
        "/Width 1 /Height 1 /ColorSpace [/Lab << /WhitePoint [1 1 1] >>] /BitsPerComponent 8",
        "/Width 1 /Height 1 /BitsPerComponent 8",
        "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 3",
        "/Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
        "/Width 0 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
    ];
    for dict in CASES {
        assert_eq!(drawn(dict, &[0]), [], "{dict}");
    }
}

#[test]
fn keeps_encoded_images_as_stored() {
    let jpeg = b"\xFF\xD8\xFF\xD9".as_slice();
    let flated = miniz_oxide::deflate::compress_to_vec_zlib(jpeg, 6);
    let cases: &[(&str, &[u8], ImageFormat, &[u8])] = &[
        ("/Filter /DCTDecode", jpeg, ImageFormat::Jpeg, jpeg),
        ("/Filter [/FlateDecode /DCTDecode]", &flated, ImageFormat::Jpeg, jpeg),
        ("/Filter /JPXDecode", b"\0\0\0\x0CjP  ", ImageFormat::Jpeg2000, b"\0\0\0\x0CjP  "),
        (
            "/Filter /JBIG2Decode /DecodeParms << /JBIG2Globals 6 0 R >>",
            b"page",
            ImageFormat::Jbig2,
            b"globalspage",
        ),
    ];
    for &(filter, data, format, stored) in cases {
        let dict = format!(
            "{IMAGE} /Width 2 /Height 3 /ColorSpace /DeviceGray /BitsPerComponent 8 {filter}"
        );
        let found = extract(&page_of(
            "/X5 Do",
            &[stream_object(&dict, data), stream_object("", b"globals")],
        ));
        let [image] = found.as_slice() else { panic!("{filter}: {found:?}") };
        assert_eq!((image.format, image.data.as_slice()), (format, stored), "{filter}");
        assert_eq!((image.width, image.height), (Some(2), Some(3)), "{filter}");
    }
}

#[test]
fn wraps_fax_scans_in_tiffs() {
    // `Decode [1 0]` turns BlackIs1 around: photometric 0 is WhiteIsZero.
    const CASES: &[(&str, u32)] = &[("", 0), ("/Decode [1 0]", 1)];
    for &(decode, photometric) in CASES {
        let dict = format!(
            "{IMAGE} /Width 1728 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 1 {decode} \
             /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 1728 /BlackIs1 true >>"
        );
        let found = extract(&page_of("/X5 Do", &[stream_object(&dict, b"G4")]));
        assert_eq!(found[0].format, ImageFormat::Tiff);
        let (tags, strip) = tiff_of(&found[0].data);
        assert_eq!(strip, b"G4");
        assert_eq!((tags[&256], tags[&259], tags[&262]), (1728, 4, photometric), "{decode:?}");
    }
}

#[test]
fn extracts_each_image_once_from_where_it_is_drawn() {
    let gray = |data: &[u8]| {
        stream_object(
            &format!("{IMAGE} /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8"),
            data,
        )
    };
    let page = |contents: u32, extra: &str, resources: &str| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {contents} 0 R {extra} \
             /Resources << /XObject << {resources} >> >> >>"
        )
        .into_bytes()
    };
    let pdf = pdf_of(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_vec(),
        page(6, "", "/A 9 0 R"),
        page(7, "", "/A 9 0 R /F 10 0 R"),
        page(8, "/Rotate 90", "/C 12 0 R"),
        stream_object("", b"q 100 0 0 50 72 600 cm /A Do Q q 10 0 0 10 0 0 cm /A Do Q"),
        stream_object(
            "",
            b"/A Do /F Do q 2 0 0 1 300 400 cm BI /W 2 /H 1 /CS /G /BPC 8 ID \x10\x20 EI Q",
        ),
        stream_object("", b"q 100 0 0 50 72 600 cm /C Do Q"),
        gray(&[1]),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Matrix [1 0 0 1 100 0] \
             /Resources << /XObject << /B 11 0 R >> >>",
            b"q 20 0 0 20 0 0 cm /B Do Q",
        ),
        stream_object(
            &format!("{IMAGE} /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8"),
            &[1, 2, 3],
        ),
        gray(&[3]),
    ]);
    let placed: Vec<(u32, [f64; 4], Vec<u8>)> = extract(&pdf)
        .into_iter()
        .map(|image| {
            let ImageLocation::Page { number, bounds: Some(Bounds { left, top, right, bottom }) } =
                image.location
            else {
                panic!("{image:?}")
            };
            (number, [left, top, right, bottom], png_of(&image.data).rows)
        })
        .collect();
    assert_eq!(
        placed,
        [
            (1, [72.0, 142.0, 172.0, 192.0], vec![1]),
            (2, [100.0, 772.0, 120.0, 792.0], vec![1, 2, 3]),
            (2, [300.0, 391.0, 302.0, 392.0], vec![0x10, 0x20]),
            (3, [600.0, 72.0, 650.0, 172.0], vec![3]),
        ],
        "the second drawing of A is left out; B is inside a form; C is on a page turned a quarter"
    );
}

#[test]
fn stops_at_limits_and_at_the_callers_errors() {
    let huge =
        format!("{IMAGE} /Width 100000 /Height 100000 /ColorSpace /DeviceRGB /BitsPerComponent 8");
    let pdf = page_of("/X5 Do", &[stream_object(&huge, &[0])]);
    let result = images(&pdf, &Limits::default().max_inflated(1 << 20), |_| Ok(()));
    assert_matches!(result, Err(Error::LimitExceeded(_)));

    let small = format!("{IMAGE} /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8");
    let pdf = page_of("/X5 Do /X6 Do", &[stream_object(&small, &[0]), stream_object(&small, &[0])]);
    let mut calls = 0;
    let result = images(&pdf, &Limits::default(), |_| {
        calls += 1;
        Err(Error::Cancelled)
    });
    assert_matches!(result, Err(Error::Cancelled));
    assert_eq!(calls, 1);
}
