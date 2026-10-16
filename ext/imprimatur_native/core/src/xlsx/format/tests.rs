use std::assert_matches;

use super::*;
use crate::style::Align;

#[test]
fn builds_formats() {
    let format = Format::new().bold().italic().align(Align::Right).border(Border::Thin);
    assert!(format.font.bold && format.font.italic && !format.font.underline);
    assert_eq!(format.align, Some(Align::Right));
    assert_eq!(format, format.clone(), "formats compare by value");
    assert_ne!(format, Format::new());
}

#[test]
fn validates_number_formats() {
    assert_eq!(Format::new().number_format("0.00%").unwrap().number_code.as_deref(), Some("0.00%"));
    for bad in [String::new(), "0".repeat(256), "0\n0".to_owned()] {
        assert_matches!(
            Format::new().number_format(bad.clone()),
            Err(Error::InvalidNumberFormat(_)),
            "{bad:?}"
        );
    }
}

#[test]
fn validates_fonts() {
    let format = Format::new().font_name("Arial").unwrap().font_size(10.5).unwrap();
    assert_eq!((format.font.name(), format.font.size().as_str()), ("Arial", "10.5"));
    let defaults = Format::new().font_name("Calibri").unwrap().font_size(11.0).unwrap();
    assert_eq!(defaults, Format::new(), "naming the default font is no change");
    assert_eq!(Format::new().font_size(409.0).unwrap().font.size(), "409");

    for size in [0.5, 409.5, 10.25, f64::NAN, f64::INFINITY] {
        assert_matches!(Format::new().font_size(size), Err(Error::InvalidFontSize(_)), "{size}");
    }
    for name in [String::new(), "x".repeat(32), "Ari\tal".to_owned()] {
        assert_matches!(
            Format::new().font_name(name.clone()),
            Err(Error::InvalidFontName(_)),
            "{name:?}"
        );
    }
}
