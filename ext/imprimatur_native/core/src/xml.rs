//! The XML emitter every written part goes through: all it has to get right
//! is escaping and well-formedness. [`reader`] parses the parts of the
//! documents being read.

pub mod reader;
#[cfg(test)]
mod tests;

/// Builds one XML part in memory.
pub struct XmlWriter {
    buf: String,
}

impl XmlWriter {
    /// Starts a part with the standalone XML declaration Office writes.
    pub fn new() -> Self {
        Self {
            buf: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n"),
        }
    }

    /// Starts a part without the declaration, as legacy VML is written.
    pub const fn bare() -> Self {
        Self { buf: String::new() }
    }

    /// Writes `<name a="v" ...>`. Attribute values are escaped.
    pub fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.tag(name, attrs);
        self.buf.push('>');
    }

    /// Writes `<name a="v" .../>`.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.tag(name, attrs);
        self.buf.push_str("/>");
    }

    pub fn close(&mut self, name: &str) {
        self.buf.push_str("</");
        self.buf.push_str(name);
        self.buf.push('>');
    }

    /// Writes escaped character data.
    pub fn text(&mut self, text: &str) {
        escape_into(&mut self.buf, text, false);
    }

    /// `<name attrs>text</name>`.
    pub fn element(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.open(name, attrs);
        self.text(text);
        self.close(name);
    }

    pub fn finish(self) -> String {
        self.buf
    }

    fn tag(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.buf.push('<');
        self.buf.push_str(name);
        for (key, value) in attrs {
            self.buf.push(' ');
            self.buf.push_str(key);
            self.buf.push_str("=\"");
            escape_into(&mut self.buf, value, true);
            self.buf.push('"');
        }
    }
}

/// Escapes markup characters. In attributes, whitespace other than a space is
/// written as a character reference too, or attribute normalization would turn
/// it into a space on read.
fn escape_into(buf: &mut String, text: &str, attribute: bool) {
    for ch in text.chars() {
        match ch {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' if attribute => buf.push_str("&quot;"),
            '\t' if attribute => buf.push_str("&#9;"),
            '\n' if attribute => buf.push_str("&#10;"),
            '\r' => buf.push_str("&#13;"),
            ch => buf.push(ch),
        }
    }
}

/// True for characters XML 1.0 cannot carry at all, not even escaped.
pub const fn is_forbidden(ch: char) -> bool {
    matches!(ch, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}
