//! `Imprimatur::Validation`: an immutable data validation rule, built and
//! checked once, then applied to any number of ranges.

use imprimatur::xlsx::{Alert, Validation as Rule};
use magnus::{Error, Ruby};

use crate::{OrRaise, argument_error, keyword};

/// The symbols `alert:` takes.
const ALERTS: [(&str, Alert); 4] = [
    ("stop", Alert::Stop),
    ("warning", Alert::Warning),
    ("information", Alert::Information),
    ("off", Alert::Off),
];

#[magnus::wrap(class = "Imprimatur::Validation", free_immediately, size)]
pub struct Validation(pub Rule);

impl Validation {
    /// Called by `Imprimatur.validation`, which owns the keyword interface
    /// and passes exactly one of `values`, `source` and `formula`.
    #[expect(clippy::too_many_arguments, reason = "one positional argument per Ruby keyword")]
    pub fn build(
        ruby: &Ruby,
        values: Option<Vec<String>>,
        source: Option<String>,
        formula: Option<String>,
        allow_blank: bool,
        dropdown: bool,
        alert: Option<String>,
        (prompt_title, prompt): (String, String),
        (error_title, error): (String, String),
    ) -> Result<Self, Error> {
        let rule = match (values, source, formula) {
            (Some(values), None, None) => Rule::list(values),
            (None, Some(source), None) => Rule::list_from(source),
            (None, None, Some(formula)) => Rule::custom(formula),
            _ => {
                let message = "a validation takes one of list: or formula:";
                return Err(argument_error(ruby, message));
            }
        }
        .or_raise(ruby)?;
        let mut rule = rule
            .allow_blank(allow_blank)
            .dropdown(dropdown)
            .prompt(prompt_title, prompt)
            .and_then(|rule| rule.error(error_title, error))
            .or_raise(ruby)?;
        if let Some(alert) = alert {
            rule = rule.alert(keyword(ruby, "alert", &alert, &ALERTS)?);
        }
        Ok(Self(rule))
    }
}
