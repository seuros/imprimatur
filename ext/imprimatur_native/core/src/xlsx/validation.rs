//! Data validation: drop-down lists and formula rules on ranges of cells
//! (ECMA-376 Part 1, 18.3.1.32).

#[cfg(test)]
mod tests;

use super::range::CellRange;
use super::text::escape;
use crate::xml::XmlWriter;
use crate::{Error, Result};

/// Excel's limit on a list typed into the rule, commas included.
const MAX_LIST: usize = 255;
/// Excel's limit on a formula.
const MAX_FORMULA: usize = 8_192;
/// Excel's limits on the titles and on the messages of prompts and alerts.
const MAX_TITLE: usize = 32;
const MAX_MESSAGE: usize = 255;

/// What happens when someone types a value the rule rejects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Alert {
    /// The value is refused.
    #[default]
    Stop,
    /// The value is kept if the user confirms it.
    Warning,
    /// The value is kept; the message only informs.
    Information,
    /// The value is kept silently: lists still offer their drop-down.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    List,
    Custom,
}

/// A rule for the cells of a range, added with
/// [`Worksheet::add_validation`](super::Worksheet::add_validation):
///
/// ```
/// use imprimatur::xlsx::{Validation, Workbook};
///
/// let mut book = Workbook::new();
/// let sheet = book.add_sheet("Orders")?;
/// let yes_no = Validation::list(["Yes", "No"])?.error("Invalid", "Pick Yes or No.")?;
/// sheet.add_validation("C2:C100".parse()?, yes_no)?;
/// sheet.add_validation("D2:D100".parse()?, Validation::list_from("Units!$A$2:$A$20")?)?;
/// # Ok::<(), imprimatur::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Validation {
    kind: Kind,
    formula: String,
    allow_blank: bool,
    dropdown: bool,
    alert: Alert,
    prompt_title: Option<String>,
    prompt: Option<String>,
    error_title: Option<String>,
    error: Option<String>,
}

impl Validation {
    const fn new(kind: Kind, formula: String) -> Self {
        Self {
            kind,
            formula,
            allow_blank: true,
            dropdown: true,
            alert: Alert::Stop,
            prompt_title: None,
            prompt: None,
            error_title: None,
            error: None,
        }
    }

    /// A drop-down of these values. Excel keeps such a list in the rule
    /// itself, so it is short: 255 characters with the commas between values.
    ///
    /// # Errors
    /// [`Error::InvalidValidation`] for no values, a value with a comma (the
    /// separator) or a control character, or a list over 255 characters.
    pub fn list<S: AsRef<str>>(values: impl IntoIterator<Item = S>) -> Result<Self> {
        let mut joined = String::new();
        for (index, value) in values.into_iter().enumerate() {
            let value = value.as_ref();
            if value.contains(',') || value.chars().any(char::is_control) {
                return Err(invalid(format!(
                    "list value {value:?} has a comma or control character"
                )));
            }
            if index > 0 {
                joined.push(',');
            }
            joined.push_str(value);
        }
        if joined.is_empty() {
            return Err(invalid("a list needs at least one value".to_owned()));
        }
        let length = joined.chars().count();
        (length <= MAX_LIST).ok_or_else(|| {
            invalid(format!("the list is {length} characters; Excel takes {MAX_LIST} at most"))
        })?;
        // A formula string literal: quoted, with its quotes doubled.
        Ok(Self::new(Kind::List, format!("\"{}\"", joined.replace('"', "\"\""))))
    }

    /// A drop-down of the values in a range, such as `Units!$A$2:$A$20`, or
    /// of what a formula returns. A leading `=` is accepted and dropped.
    ///
    /// # Errors
    /// [`Error::InvalidValidation`] when empty or over 8,192 characters.
    pub fn list_from(source: impl Into<String>) -> Result<Self> {
        Ok(Self::new(Kind::List, formula(source.into())?))
    }

    /// Accepts a value when the formula is true. References in it are
    /// relative to the top-left cell of the range, as in Excel: on `B2:B9`,
    /// `LEN(B2)<=100` checks each cell's own length.
    ///
    /// # Errors
    /// As [`list_from`](Self::list_from).
    pub fn custom(formula_text: impl Into<String>) -> Result<Self> {
        Ok(Self::new(Kind::Custom, formula(formula_text.into())?))
    }

    /// Whether an empty cell passes, true by default.
    #[must_use]
    pub const fn allow_blank(mut self, allow: bool) -> Self {
        self.allow_blank = allow;
        self
    }

    /// Whether a list shows its in-cell drop-down arrow, true by default.
    #[must_use]
    pub const fn dropdown(mut self, shown: bool) -> Self {
        self.dropdown = shown;
        self
    }

    #[must_use]
    pub const fn alert(mut self, alert: Alert) -> Self {
        self.alert = alert;
        self
    }

    /// A note shown while a cell of the range is selected; either part may
    /// be empty.
    ///
    /// # Errors
    /// [`Error::InvalidValidation`] for a title over 32 characters or a
    /// message over 255.
    pub fn prompt(mut self, title: impl Into<String>, message: impl Into<String>) -> Result<Self> {
        self.prompt_title = text("prompt title", title.into(), MAX_TITLE)?;
        self.prompt = text("prompt", message.into(), MAX_MESSAGE)?;
        Ok(self)
    }

    /// The title and message of the alert a rejected value raises; Excel
    /// uses its own when they are empty.
    ///
    /// # Errors
    /// As [`prompt`](Self::prompt).
    pub fn error(mut self, title: impl Into<String>, message: impl Into<String>) -> Result<Self> {
        self.error_title = text("error title", title.into(), MAX_TITLE)?;
        self.error = text("error", message.into(), MAX_MESSAGE)?;
        Ok(self)
    }

    /// `<dataValidation>` for `range`.
    pub(crate) fn write(&self, xml: &mut XmlWriter, range: &CellRange) {
        let sqref = range.to_string();
        let mut attrs = vec![(
            "type",
            match self.kind {
                Kind::List => "list",
                Kind::Custom => "custom",
            },
        )];
        match self.alert {
            Alert::Stop | Alert::Off => {}
            Alert::Warning => attrs.push(("errorStyle", "warning")),
            Alert::Information => attrs.push(("errorStyle", "information")),
        }
        if self.allow_blank {
            attrs.push(("allowBlank", "1"));
        }
        // Inverted in the file format: showDropDown="1" hides the arrow.
        if self.kind == Kind::List && !self.dropdown {
            attrs.push(("showDropDown", "1"));
        }
        if self.prompt_title.is_some() || self.prompt.is_some() {
            attrs.push(("showInputMessage", "1"));
        }
        if self.alert != Alert::Off {
            attrs.push(("showErrorMessage", "1"));
        }
        let texts = [
            ("errorTitle", &self.error_title),
            ("error", &self.error),
            ("promptTitle", &self.prompt_title),
            ("prompt", &self.prompt),
        ]
        .map(|(name, value)| (name, value.as_deref().map(escape)));
        for (name, value) in &texts {
            if let Some(value) = value {
                attrs.push((name, value));
            }
        }
        attrs.push(("sqref", &sqref));
        xml.open("dataValidation", &attrs);
        xml.element("formula1", &[], &escape(&self.formula));
        xml.close("dataValidation");
    }
}

/// `<dataValidations>`, nothing when there are none.
pub fn write_all(xml: &mut XmlWriter, validations: &[(CellRange, Validation)]) {
    if validations.is_empty() {
        return;
    }
    let count = validations.len().to_string();
    xml.open("dataValidations", &[("count", &count)]);
    for (range, validation) in validations {
        validation.write(xml, range);
    }
    xml.close("dataValidations");
}

const fn invalid(reason: String) -> Error {
    Error::InvalidValidation(reason)
}

fn formula(mut source: String) -> Result<String> {
    if source.starts_with('=') {
        source.remove(0);
    }
    let length = source.chars().count();
    if source.trim().is_empty() || length > MAX_FORMULA {
        return Err(invalid(format!(
            "a formula must be 1 to {MAX_FORMULA} characters, not {length}"
        )));
    }
    Ok(source)
}

/// `None` for empty text, which Excel treats as absent.
fn text(what: &str, value: String, limit: usize) -> Result<Option<String>> {
    let length = value.chars().count();
    (length <= limit).ok_or_else(|| {
        invalid(format!("the {what} is {length} characters; Excel takes {limit} at most"))
    })?;
    Ok((!value.is_empty()).then_some(value))
}
