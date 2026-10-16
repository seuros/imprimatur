use std::assert_matches;

use super::*;

#[test]
fn parses_hex_colors() {
    assert_eq!(Color::from_hex("#1F4E79").unwrap(), Color::rgb(0x1F_4E_79));
    assert_eq!(Color::from_hex("ffffff").unwrap(), Color::WHITE);
    assert_eq!(Color::rgb(0x1234_5678).argb(), "FF345678", "bits above 24 are ignored");
    for bad in ["", "#", "#FFF", "#GGGGGG", "#1234567", "+12345", "#+1234"] {
        assert_matches!(Color::from_hex(bad), Err(Error::InvalidColor(_)), "{bad:?}");
    }
}
