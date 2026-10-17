# imprimatur

Document authoring with no third-party format stack: the XML and the zip
container are written here, on `miniz_oxide` (deflate) and `crc32fast`.
XLSX workbooks, and PDF and DOCX from one document model.
Output is deterministic: the same document always gives the same bytes.

```rust
use imprimatur::xlsx::{Cell, Color, DateTime, Format, Workbook};

let header = Format::new().bold().fill(Color::from_hex("#1F4E79")?).font_color(Color::WHITE);
let mut book = Workbook::new();
let sheet = book.add_sheet("Budget")?;
sheet.append_row_with_format(["Line item", "Promised", "Delivered"], &header)?;
sheet.append_row([Cell::from("High-speed rail"), 1_000_000_000.into(), 0.into()])?;
sheet.append_row([Cell::from("Total"), Cell::formula("SUM(B2:B2)"), Cell::formula("SUM(C2:C2)")])?;
sheet.write(5, 0, "written at A6")?; // zero-based row and column
sheet.write(6, 0, DateTime::date(2026, 10, 15)?)?; // shows as yyyy-mm-dd
sheet.set_column_width(0, 24.0)?.set_right_to_left(false);

book.save("budget.xlsx")?;          // or to_bytes(), or write_to(impl Write)
# std::fs::remove_file("budget.xlsx")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

```rust
use imprimatur::doc::{Document, Paragraph, Run, Table};

let mut doc = Document::new();
doc.set_page_numbers(true);
doc.heading(1, "Quarterly report")?;
doc.push_paragraph(Paragraph::new().text("Revenue was ").run(Run::new("up 12%").bold()))?;
doc.table(Table::new([["Region", "Q1"], ["North", "12.5"]]).header())?;
let pdf = doc.to_pdf()?; // Helvetica, Windows-1252 text
let docx = doc.to_docx()?; // any script
# Ok::<(), imprimatur::Error>(())
```

```rust
use imprimatur::xlsx::{Alert, Comment, Format, Validation, VerticalAlign, Workbook};

let header = Format::new().font_name("Arial")?.font_size(12.0)?.bold().valign(VerticalAlign::Center);
let mut book = Workbook::new();
let sheet = book.add_sheet("Items")?;
sheet.append_row_with_format(["Name", "Unit"], &header)?;
sheet.set_comment(0, 0, Comment::new("Required.")?.author("Instructions"))?;
sheet.add_validation("B2:B1000".parse()?, Validation::list(["g", "kg", "ea"])?)?;
let short = Validation::custom("LEN(A2)<=100")?.alert(Alert::Warning).prompt("Name", "Short")?;
sheet.add_validation("A2:A1000".parse()?, short)?;
# Ok::<(), imprimatur::Error>(())
```

Everything Excel would refuse is an `Error` before a byte is written: invalid
or duplicate sheet names, positions beyond 1,048,576 rows or 16,384 columns,
text over 32,767 characters, formulas over 8,192, NaN and infinity, font
sizes Excel cannot set, inline validation lists over 255 characters, and
validations whose ranges overlap. A rejected row writes nothing.

Reading goes the other way, any PDF, XLSX, XLS, ODS or DOCX to Markdown, within
limits (input size, decompressed size, nesting, time, a cancel flag):

```rust
use imprimatur::read::{Limits, markdown};

# let mut book = imprimatur::xlsx::Workbook::new();
# book.add_sheet("Notes")?.append_row(["Hello"])?;
# let document = book.to_bytes()?;
let text = markdown(&document, &Limits::default())?;
assert!(text.starts_with("## Notes"));
# Ok::<(), imprimatur::Error>(())
```

Markdown is one rendering of what a read finds. `content` gives the
structure itself: a PDF's pages of headings and paragraphs with their
position on the page, a workbook's sheets of typed cells (number, date,
bool, text, error, formula) and comments, a Word document's blocks with
their bold, italic and linked stretches. `images` hands over every image,
as a file: JPEG and JPEG 2000 as stored, fax scans as TIFF, raw pixels as
PNG, an Office document's media as they are.

```rust
use imprimatur::read::{Content, Limits, Value, content, images};

# let mut book = imprimatur::xlsx::Workbook::new();
# book.add_sheet("Notes")?.append_row([imprimatur::xlsx::Cell::from(12.5)])?;
# let document = book.to_bytes()?;
let Content::Xlsx(sheets) = content(&document, &Limits::default())? else { panic!() };
let cell = &sheets[0].cells.cells()[0];
assert_eq!((cell.reference(), cell.value), ("A1".to_owned(), Value::Number(12.5)));

let mut found = Vec::new();
images(&document, &Limits::default(), |image| {
    found.push((image.format.extension(), image.data));
    Ok(())
})?;
assert!(found.is_empty(), "no pictures in this one");
# Ok::<(), imprimatur::Error>(())
```

With the `fonts` feature, `font::TrueTypeFont::from_bytes` loads borrowed
static TrueType data and exposes Unicode glyph IDs, horizontal advances and
raw `hhea` line metrics in font units. It checks the required tables and
outline offsets, but is not a full font sanitizer. Collections, WOFF, CFF and
variable fonts are not supported. This is a Rust font-data API only. The
internal PDF writer can serialize TrueType font resources with every glyph
and Unicode text mappings; document font selection, shaping and bidirectional
layout still need to connect to them. Document PDF output uses Helvetica.

Dependencies: `miniz_oxide`, `crc32fast` and `thiserror`; reading encrypted
PDFs adds `md-5`, `sha2` and `aes`.

Three features add what the core does not need: `json` (`read::json`, the
schema the command line and the Ruby gem print, through `serde_json`),
`archive` (`archive::{Writer, Archive}`: zip archives of any files, with
times, comments and ZIP64, through the `zip` crate) and `fonts`
(`font::TrueTypeFont`: static TrueType font data, through `read-fonts`).

## License

BSD 3-Clause
