use crate::style::{Align, Color, TextDirection};

/// A stretch of text with one look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) color: Option<Color>,
}

impl Run {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), bold: false, italic: false, underline: false, color: None }
    }

    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    #[must_use]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Runs of text laid out together. Left aligned unless told otherwise, or
/// right aligned when right to left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paragraph {
    pub(crate) runs: Vec<Run>,
    pub(crate) align: Option<Align>,
    pub(crate) direction: Option<TextDirection>,
}

impl Paragraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends plain text.
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text))
    }

    /// Appends bold text.
    #[must_use]
    pub fn bold(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text).bold())
    }

    /// Appends italic text.
    #[must_use]
    pub fn italic(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text).italic())
    }

    #[must_use]
    pub fn run(mut self, run: Run) -> Self {
        self.runs.push(run);
        self
    }

    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    #[must_use]
    pub const fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    #[must_use]
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The alignment the paragraph is laid out with.
    pub(crate) fn effective_align(&self) -> Align {
        self.align.unwrap_or(match self.direction {
            Some(TextDirection::RightToLeft) => Align::Right,
            _ => Align::Left,
        })
    }
}
