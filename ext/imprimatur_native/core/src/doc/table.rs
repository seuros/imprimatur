/// A grid of text cells; columns share the page width equally. Short rows
/// are padded with empty cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) header: bool,
}

impl Table {
    pub fn new<R, C>(rows: impl IntoIterator<Item = R>) -> Self
    where
        R: IntoIterator<Item = C>,
        C: Into<String>,
    {
        let rows = rows.into_iter().map(|row| row.into_iter().map(Into::into).collect()).collect();
        Self { rows, header: false }
    }

    /// Makes the first row a header: bold, shaded, and repeated on every
    /// page the table spans.
    #[must_use]
    pub const fn header(mut self) -> Self {
        self.header = true;
        self
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The widest row's cell count.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub(crate) const fn has_header(&self) -> bool {
        self.header
    }
}
