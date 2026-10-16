use std::io;

/// Everything that can go wrong while building or writing a document.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid sheet name {name:?}: {reason}")]
    InvalidSheetName { name: String, reason: &'static str },

    #[error("a sheet named {0:?} already exists (sheet names are case-insensitive)")]
    DuplicateSheetName(String),

    #[error("row {0} is out of range (rows are 0..1048576)")]
    RowOutOfRange(u64),

    #[error("column {0} is out of range (columns are 0..16384)")]
    ColumnOutOfRange(u64),

    #[error("text of {0} characters is too long for a cell (the limit is 32767)")]
    TextTooLong(usize),

    #[error("a formula of {0} characters is too long (the limit is 8192)")]
    FormulaTooLong(usize),

    #[error("{0} is not a finite number; cells cannot hold NaN or infinity")]
    NonFiniteNumber(f64),

    #[error("invalid color {0:?}: expected #RRGGBB")]
    InvalidColor(String),

    #[error(
        "invalid number format {0:?}: it must be 1 to 255 characters, without control characters"
    )]
    InvalidNumberFormat(String),

    #[error("{0} is outside Excel's dates (1900-01-01 to 9999-12-31)")]
    DateOutOfRange(String),

    #[error("column width {0} is out of range (0 to 255 characters)")]
    InvalidColumnWidth(f64),

    #[error("invalid font name {0:?}: it must be 1 to 31 characters, without control characters")]
    InvalidFontName(String),

    #[error("font size {0} is out of range (1 to 409 points, in steps of 0.5)")]
    InvalidFontSize(f64),

    #[error("invalid cell range {0:?}: expected a reference such as B2 or B2:D9")]
    InvalidRange(String),

    #[error("invalid data validation: {0}")]
    InvalidValidation(String),

    #[error("the data validation on {0} overlaps one already on the sheet")]
    OverlappingValidation(String),

    #[error("too many distinct formats (Excel's limit is 64000 per workbook)")]
    TooManyFormats,

    #[error("heading level {0} is out of range (1 to 3)")]
    InvalidHeadingLevel(u8),

    #[error("text contains the control character U+{0:04X}; only newlines and tabs are allowed")]
    ControlCharacter(u32),

    #[error("a table needs at least one row and one column")]
    EmptyTable,

    #[error(
        "{character:?} (U+{code:04X}) cannot be written with the PDF standard font {font}: \
         only Windows-1252 (Latin) text is supported until embedded fonts land",
        code = u32::from(*character)
    )]
    UnsupportedCharacter { character: char, font: &'static str },

    #[error("a table row needs {0:.0}pt, more than a whole page holds")]
    RowTooTall(f64),

    #[error("a workbook needs at least one sheet")]
    EmptyWorkbook,

    #[error("archive too large: {0} (zip64 is not supported yet)")]
    ArchiveTooLarge(&'static str),

    #[error("cannot read this document: {0}")]
    UnsupportedDocument(String),

    #[error("malformed {format}: {reason}")]
    Malformed { format: &'static str, reason: String },

    #[error("the document exceeds the {0} limit")]
    LimitExceeded(&'static str),

    #[error("reading the document was cancelled")]
    Cancelled,

    #[error("the PDF has no text layer: its pages are images, which need OCR")]
    NoTextLayer,

    #[error("the PDF is encrypted with a password")]
    PasswordRequired,

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl Error {
    pub(crate) fn malformed(format: &'static str, reason: impl Into<String>) -> Self {
        Self::Malformed { format, reason: reason.into() }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
