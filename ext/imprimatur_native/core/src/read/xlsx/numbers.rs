//! How XLSX number formats show numbers: which formats are dates, times
//! or percentages, and Excel's General format.

use std::fmt::Write as _;

use crate::Result;
use crate::read::Budget;
use crate::read::content::Value;
use crate::xlsx::DateTime;
use crate::xml::reader::{Event, XmlReader};

/// How a number format shows a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    General,
    Date,
    Time,
    DateTime,
    Percent,
}

/// How each cell format (`cellXfs`, by index) shows numbers.
pub fn styles(text: &str, budget: &Budget<'_>) -> Result<Vec<Shown>> {
    let mut xml = XmlReader::new(text, budget.max_depth());
    let (mut custom, mut shown, mut in_cell_formats) = (Vec::new(), Vec::new(), false);
    while let Some(event) = xml.next()? {
        match event {
            Event::Start(tag) => match tag.name() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (tag.attr("numFmtId"), tag.attr("formatCode")) {
                        custom.push((id.parse::<u32>().unwrap_or(u32::MAX), classify(&code)));
                    }
                }
                "cellXfs" => in_cell_formats = true,
                "xf" if in_cell_formats => {
                    let id =
                        tag.attr("numFmtId").and_then(|id| id.parse::<u32>().ok()).unwrap_or(0);
                    let custom =
                        custom.iter().find(|&&(custom, _)| custom == id).map(|&(_, shown)| shown);
                    shown.push(custom.unwrap_or_else(|| built_in(id)));
                }
                _ => {}
            },
            Event::End("cellXfs") => in_cell_formats = false,
            _ => {}
        }
    }
    Ok(shown)
}

/// Excel's built-in number formats (ECMA-376 Part 1, 18.8.30), with the
/// East Asian date formats.
const fn built_in(id: u32) -> Shown {
    match id {
        9 | 10 => Shown::Percent,
        14..=17 | 27..=31 | 34..=36 | 50..=58 => Shown::Date,
        18..=21 | 32 | 33 | 45..=47 => Shown::Time,
        22 => Shown::DateTime,
        _ => Shown::General,
    }
}

/// What a custom format code shows, from its first section: date and time
/// tokens, outside quoted text, escapes and `[...]` (colors, locales;
/// `[h]` is elapsed time).
pub fn classify(code: &str) -> Shown {
    let (mut date, mut time, mut month, mut percent) = (false, false, false, false);
    let mut chars = code.chars();
    while let Some(ch) = chars.next() {
        match ch.to_ascii_lowercase() {
            ';' => break,
            '"' => {
                chars.by_ref().find(|&ch| ch == '"');
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                let inner: String = chars.by_ref().take_while(|&ch| ch != ']').collect();
                time |= matches!(inner.to_ascii_lowercase().chars().next(), Some('h' | 'm' | 's'));
            }
            'y' | 'd' => date = true,
            'h' | 's' => time = true,
            'm' => month = true,
            '%' => percent = true,
            _ => {}
        }
    }
    match (date || (month && !time), time) {
        (true, true) => Shown::DateTime,
        (true, false) => Shown::Date,
        (false, true) => Shown::Time,
        (false, false) if percent => Shown::Percent,
        (false, false) => Shown::General,
    }
}

/// Writes `number` as `shown` says; returns the value, a date when it is
/// shown as one. (Writing to a String cannot fail: the results of `write!`
/// here are dropped on purpose.)
pub fn push_number(
    out: &mut String,
    number: f64,
    shown: Shown,
    date1904: bool,
    scratch: &mut String,
) -> Value {
    let date = || DateTime::from_serial(number, date1904);
    match shown {
        Shown::Date if let Some(date) = date() => {
            let date = date.date_only();
            // Writing to a String cannot fail.
            let _ = write!(out, "{date}");
            return Value::Date(date);
        }
        Shown::DateTime if let Some(date) = date() => {
            // Writing to a String cannot fail.
            let _ = write!(out, "{date}");
            return Value::Date(date);
        }
        Shown::Time => {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0 to 86400 after rem_euclid"
            )]
            let seconds = (number.fract() * 86_400.0).round().rem_euclid(86_400.0) as u32;
            // Writing to a String cannot fail.
            let _ =
                write!(out, "{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
        }
        Shown::Percent => {
            push_general(out, number * 100.0, scratch);
            out.push('%');
        }
        _ => push_general(out, number, scratch),
    }
    Value::Number(number)
}

/// A number as Excel's General format shows it: 15 significant digits.
pub fn push_general(out: &mut String, number: f64, scratch: &mut String) {
    #[expect(clippy::cast_possible_truncation, reason = "whole and under 1e15: exact in i64")]
    if number.fract() == 0.0 && number.abs() < 1e15 {
        // Writing to a String cannot fail.
        let _ = write!(out, "{}", number as i64);
        return;
    }
    scratch.clear();
    // Writing to a String cannot fail.
    let _ = write!(scratch, "{number:.14e}");
    let rounded = scratch.parse::<f64>().unwrap_or(number);
    // Writing to a String cannot fail.
    let _ = write!(out, "{rounded}");
}
