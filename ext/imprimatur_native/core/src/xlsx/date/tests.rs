use std::assert_matches;

use super::*;

#[test]
fn matches_excel_serials() {
    // Values as Excel's =DATEVALUE() gives them, including the phantom 1900-02-29.
    const CASES: &[((u16, u8, u8), f64)] = &[
        ((1900, 1, 1), 1.0),
        ((1900, 2, 28), 59.0),
        ((1900, 3, 1), 61.0),
        ((1970, 1, 1), 25_569.0),
        ((2000, 2, 29), 36_585.0),
        ((2023, 3, 15), 45_000.0),
        ((9999, 12, 31), 2_958_465.0),
    ];
    for ((year, month, day), serial) in CASES {
        let date = DateTime::date(*year, *month, *day).unwrap();
        assert_eq!(date.serial(), *serial, "{date}");
    }
}

#[test]
fn adds_the_time_of_day_as_a_fraction() {
    let noon = DateTime::date(2023, 3, 15).unwrap().at(12, 0, 0, 0).unwrap();
    assert_eq!(noon.serial(), 45_000.5);
    let late = DateTime::date(2023, 3, 15).unwrap().at(18, 30, 15, 500_000_000).unwrap();
    assert!((late.serial() - (45_000.0 + 66_615.5 / 86_400.0)).abs() < 1e-9);
    assert!(late.has_time() && !DateTime::date(2023, 3, 15).unwrap().has_time());
    assert_eq!(late.to_string(), "2023-03-15 18:30:15.500000000");
}

#[test]
fn rejects_dates_excel_cannot_hold() {
    let dates = [
        (1899, 12, 31),
        (10_000, 1, 1),
        (2023, 2, 29),
        (1900, 2, 29),
        (2023, 13, 1),
        (2023, 4, 31),
        (2023, 1, 0),
    ];
    for (year, month, day) in dates {
        assert_matches!(
            DateTime::date(year, month, day),
            Err(Error::DateOutOfRange(_)),
            "{year}-{month}-{day}"
        );
    }
    let date = DateTime::date(2024, 2, 29).unwrap();
    for (hour, minute, second, nano) in
        [(24, 0, 0, 0), (0, 60, 0, 0), (23, 59, 60, 0), (0, 0, 0, 1_000_000_000)]
    {
        assert_matches!(date.at(hour, minute, second, nano), Err(Error::DateOutOfRange(_)));
    }
}

#[test]
fn reads_serials_back() {
    let dates = [
        DateTime::date(1900, 1, 1).unwrap(),
        DateTime::date(1900, 2, 28).unwrap(),
        DateTime::date(1900, 3, 1).unwrap(),
        DateTime::date(2023, 3, 15).unwrap().at(18, 30, 5, 0).unwrap(),
        DateTime::date(9999, 12, 31).unwrap().at(23, 59, 59, 0).unwrap(),
    ];
    for date in dates {
        assert_eq!(DateTime::from_serial(date.serial(), false), Some(date), "{date}");
    }
    assert_eq!(DateTime::from_serial(60.0, false), None, "Lotus's 1900-02-29");
    assert_eq!(
        DateTime::from_serial(0.0, true),
        Some(DateTime::date(1904, 1, 1).unwrap()),
        "the 1904 epoch"
    );
    assert_eq!(
        DateTime::from_serial(45_000.999_999_9, false),
        Some(DateTime::date(2023, 3, 16).unwrap()),
        "rounds to the second"
    );
    for serial in [-1.0, f64::NAN, 2_958_466.0] {
        assert_eq!(DateTime::from_serial(serial, false), None, "{serial}");
    }
}
