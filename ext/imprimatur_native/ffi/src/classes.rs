//! Classes resolved once at load and pinned.

use std::sync::OnceLock;

use magnus::{Error, ExceptionClass, RClass, RModule, Ruby, prelude::*};
use matryona_machines::ruby::Class;

pub struct Classes {
    pub formula: Class,
    pub error: Class,
    pub no_text_layer: Class,
    pub password_required: Class,
    pub unsupported_document: Class,
    pub limit_exceeded: Class,
}

matryona_machines::registry! {
    static CLASSES: Classes = "imprimatur";
    pub fn classes;
}

pub fn resolve(ruby: &Ruby, imprimatur: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, imprimatur, name);
    let classes = Classes {
        formula: class("Formula")?,
        error: class("Error")?,
        no_text_layer: class("NoTextLayer")?,
        password_required: class("PasswordRequired")?,
        unsupported_document: class("UnsupportedDocument")?,
        limit_exceeded: class("LimitExceeded")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

pub fn exception(ruby: &Ruby, class: Class) -> ExceptionClass {
    ExceptionClass::from_value(class.get(ruby).as_value())
        .unwrap_or_else(|| ruby.exception_standard_error())
}

/// `Date` is only there once something has required 'date'.
pub fn date(ruby: &Ruby) -> Result<Option<RClass>, Error> {
    static DATE: OnceLock<Class> = OnceLock::new();
    if let Some(class) = DATE.get() {
        return Ok(Some(class.get(ruby)));
    }
    let object = ruby.class_object();
    if !object.funcall::<_, _, bool>("const_defined?", ("Date",))? {
        return Ok(None);
    }
    let class: RClass = object.const_get("Date")?;
    Ok(Some(DATE.get_or_init(|| Class::pin(ruby, class)).get(ruby)))
}
