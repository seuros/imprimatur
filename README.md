# imprimatur

*Imprimatur*: "let it be printed", the stamp a censor put on a book before it
could go to press. This one stamps spreadsheets, PDFs and Word documents. No heresy reaches the printer: an invalid sheet name, a NaN or
a 40,000-character cell is refused at the door, not discovered by Excel's
"We found a problem with some content".

- **A Ruby DSL over a native core.** The core is also published on its own,
  as `imprimatur` on crates.io; the gem wraps it.
- **No third-party format stack.** The XML writer and the zip container are
  written here, from ECMA-376 and the zip specification; only deflate
  (`miniz_oxide`) and CRC-32 (`crc32fast`) come from crates.
- **Deterministic.** The same document always gives the same bytes.
- **Native only.** CRuby 4.0+. No pure Ruby fallback, no JRuby, no TruffleRuby.

```ruby
require "imprimatur"

Imprimatur.xlsx("budget.xlsx") do |book|
  book.sheet("Budget") do |sheet|
    sheet.row "Line item", "Promised", "Delivered"
    sheet.row "High-speed rail", 1_000_000_000, 0
    sheet.row "Bridge to nowhere", 398_000_000, 398_000_000
    sheet.row "Total", Imprimatur.formula("SUM(B2:B3)"), Imprimatur.formula("SUM(C2:C3)")
  end
end
```

## Installation

```ruby
gem "imprimatur"
```

Precompiled gems ship for Linux (x86_64 and aarch64, glibc 2.34+ and musl),
macOS 13+ (arm64, x86_64) and FreeBSD 15.1 (amd64): nothing to compile.
Anywhere else, the source gem compiles the extension on install, with the
toolchain from [rustup.rs](https://rustup.rs) (1.99+).

## Workbooks

```ruby
bytes = Imprimatur.xlsx do |book|       # no path: returns the .xlsx bytes
  sheet = book.sheet("Sales")
  sheet.row "Region", "Q1", "On target"
  sheet.row "North", 12.5, true
  sheet << ["South", nil, false]         # nil leaves a cell empty
  sheet.write(9, 2, "C10")               # zero-based row and column
  sheet.next_row                         # => 10
end

send_data bytes, filename: "sales.xlsx", type: :xlsx
```

| Ruby value | Cell |
|---|---|
| `String` | text, up to 32,767 characters |
| `Integer` | number, up to 2\*\*53 (beyond, Excel would store a different number: `RangeError`) |
| `Float` | number; NaN and infinity are refused |
| `BigDecimal`, `Rational` | number, as its `to_f`: Excel keeps every number as a double |
| `true`, `false` | boolean |
| `Imprimatur.formula("SUM(A1:A3)")` | formula, computed when the workbook opens |
| `Date`, `Time`, `DateTime` | date; its wall-clock fields, no time zone (1900-01-01 to 9999-12-31) |
| `ActiveSupport::TimeWithZone` | as `Time`, the wall clock of its zone |
| `nil` | nothing |

Dates without a number format of their own show as `yyyy-mm-dd`, or
`yyyy-mm-dd hh:mm:ss` with a time of day.

Anything else raises `TypeError`. Excel's own limits raise `Imprimatur::Error`:
sheet names (31 characters, no `[ ] : * ? / \`, unique ignoring case, not
`History`), 1,048,576 rows, 16,384 columns. A rejected row writes nothing.

## Styles

```ruby
header = Imprimatur.style(bold: true, color: "#FFFFFF", fill: "#1F4E79", align: :center, border: :thin)
money  = Imprimatur.style(number_format: "#,##0.00")

Imprimatur.xlsx("report.xlsx") do |book|
  book.sheet("Report") do |sheet|
    sheet.column_widths 24, 14                 # characters; nil skips a column
    sheet.row "Line item", "Amount", style: header
    sheet.row "Consulting", nil
    sheet.write(1, 1, 98_000, style: money)
  end
end
```

`Imprimatur.style` takes `number_format:`, `bold:`, `italic:`, `underline:`,
`color:` and `fill:` (`"#RRGGBB"`), `font:` (`"Arial"`; Calibri by default),
`size:` (points, in steps of 0.5; 11 by default), `align:` (`:left`,
`:center`, `:right`), `valign:` (`:top`, `:center`, `:bottom`), `direction:`
(`:ltr`, `:rtl`), `wrap:` and `border:` (`:thin`, `:medium`, `:thick`). A
style is checked once, when it is built, and every cell using it shares one
entry in the workbook.

`style:` also takes an Array, one style (or `nil`) per column from A; the
columns past its end are left unstyled:

```ruby
sheet.row "", "", "", "Notes", style: [yellow] * 3
```

## Validation and comments

```ruby
yes_no = Imprimatur.validation(list: %w[Yes No])     # built once, shared

book.sheet("Items") do |sheet|
  sheet.row "Name", "Unit", "Supplier", "Raw material"
  sheet.comment "A1", "Required. Up to 100 characters.", author: "Instructions"
  sheet.validate "A2:A1000", formula: "LEN(A2)<=100", alert: :warning, prompt: "Up to 100 characters"
  sheet.validate "B2:B1000", list: %w[g kg m l ea]
  sheet.validate "C2:C1000", list: "Suppliers!$A$2:$A$500",
                             error_title: "Unknown supplier", error: "Pick one from the list."
  sheet.validate "D2:D1000", yes_no
end
```

`list:` is the drop-down's values, or a range holding them (another sheet's
too). Values typed into the rule stay under Excel's 255 characters, commas
between them included, and cannot contain a comma; longer lists belong in a
range. `formula:` accepts a value when it is true, its references relative to
the range's top-left cell. `alert:` decides what an invalid value meets:
`:stop` (refused, the default), `:warning`, `:information`, or `:off` (accepted;
a list still offers its drop-down). `prompt:` and `prompt_title:` show while a
cell is selected, `error:` and `error_title:` in the alert. Ranges are A1 text;
one overlapping a range already validated is refused, since Excel applies a
single rule per cell.

A comment is an Excel note: hidden until the pointer rests on its cell, or
shown all the time with `visible: true`. The cell need not hold a value.

## Right to left

```ruby
rtl = Imprimatur.style(direction: :rtl)

book.sheet("مبيعات") do |sheet|                        # Arabic
  sheet.right_to_left!                                 # column A on the right
  sheet.row "المنطقة", "الشمال", style: rtl
end

book.sheet("מכירות") do |sheet|                        # Hebrew
  sheet.right_to_left!
  sheet.row "אזור", "צפון", style: rtl
end
```

Text in any right-to-left language needs nothing special: Hebrew, Yiddish,
Arabic, Persian, Urdu, Pashto, Sindhi, Kurdish (Sorani), Uyghur, Syriac,
Dhivehi (Thaana), N'Ko, Adlam and the rest. `right_to_left!` lays the sheet
out the way Excel does for them, and `direction: :rtl` fixes a cell's reading
order instead of letting Excel infer it from its first letter.

Not yet: merged cells, frozen panes, streaming for very large sheets.

## Documents: PDF and DOCX

One document, two formats:

```ruby
doc = Imprimatur::Document.new(page_size: :a4, page_numbers: true)
doc.heading "Quarterly report"
doc.heading "Sales", level: 2
doc.paragraph do |p|
  p.text "Revenue was "
  p.text "up 12%", bold: true, color: "#1F7A1F"
  p.text " this quarter."
end
doc.paragraph "Figures are unaudited.", italic: true, align: :center
doc.table [["Region", "Q1"], ["North", 12_500.5]], header: true
doc.page_break
doc.paragraph "Appendix."

doc.save_pdf("report.pdf")
doc.save_docx("report.docx")

Imprimatur.pdf("memo.pdf") { it.paragraph "Or in one go." }    # Imprimatur.docx alike
```

- Headings 1 to 3 (real Word heading styles in DOCX), paragraphs of runs
  (bold, italic, underline, color), alignment, `\n` line breaks and tabs.
- Tables: equal columns, borders, a shaded header row that repeats on every
  page; cells are converted with `to_s`.
- Page breaks, A4 or Letter, `1 / N` page numbers.
- `direction: :rtl` makes a right-to-left paragraph (right aligned, and
  `w:bidi` in DOCX).

**PDF text is Latin only for now.** PDFs use the standard Helvetica fonts,
which carry Windows-1252: English and Western European text, curly quotes,
dashes, the euro sign. Anything else (Arabic, Hebrew, Greek, CJK, emoji) raises
`Imprimatur::Error` naming the character, never a `?`. DOCX carries any
script. Embedded TrueType fonts, and with them every script and proper RTL in
PDF, are the next step.

## Reading: documents as Markdown

The other direction: a PDF, XLSX or DOCX printed as Markdown, for search
indexes and language models.

```ruby
Imprimatur.markdown("audit.pdf")              # a path
Imprimatur.markdown(upload.download)          # bytes: the format is in the content
Imprimatur.markdown(File.open("sales.xlsx"))  # an IO

begin
  Imprimatur.markdown(scan)
rescue Imprimatur::NoTextLayer
  # scanned pages: images, which need OCR
end
```

- **PDF**: the text layer, placed glyph by glyph (fonts, encodings and
  ToUnicode maps, forms, rotated pages), in reading order (right-to-left
  lines too: Hebrew, Arabic, Persian, Urdu, Syriac, Dhivehi, N'Ko, Adlam,
  their vowel marks kept on their letters), with headings by font size, hyphenated line breaks rejoined,
  presentation forms (`ﬁ`, Arabic contextual forms) as plain letters, and
  running headers and footers left out. Pages after the first start with
  `<!-- page N -->`. Encrypted PDFs open when the user password is empty, as
  it is for documents that only restrict printing or copying (RC4, AES-128,
  AES-256).
- **XLSX**: each sheet as a table under its name, values as Excel shows them
  (dates as dates, percentages, 15 significant digits), formulas without a
  cached result as `=FORMULA`, then the sheet's comments.
- **DOCX**: headings (from styles and outline levels), bulleted and numbered
  lists, bold, italic, links, tables, text boxes; deleted text and field
  codes left out.

Escaping is minimal: only what would break a table (`|`, line breaks). The
text is meant to be searched and read, and `snake\_case` would be noise.

Documents are untrusted input, so reads are bounded: `max_bytes:` (512 MiB),
`max_inflated:` (1 GiB of decompressed data, zip bombs included) and
`timeout:` (60 seconds), plus a nesting limit; past one,
`Imprimatur::LimitExceeded`. Reading runs without the GVL: other threads and
fibers keep running, and `Timeout` or `Thread#raise` cancels it.
`Imprimatur::PasswordRequired` and `Imprimatur::UnsupportedDocument` (legacy
`.xls` and `.doc`, `.xlsb`, presentations) are the other errors to expect.

### Structure and images

Markdown is one rendering of what a read finds. `Imprimatur.content` gives
the structure itself, the same as `imprimatur --json md` prints: a PDF's
pages of headings and paragraphs with their position, a workbook's typed
cells and comments, a Word document's blocks with their bold, italic and
linked stretches.

```ruby
sheet = Imprimatur.content("sales.xlsx")[:sheets].first
sheet[:rows][1][:cells]
# => [{cell: "A2", type: "text", value: "North"},
#     {cell: "B2", type: "number", value: 12.5, text: "12.5"}]

Imprimatur.content("audit.pdf")[:pages].first[:blocks].first
# => {type: "heading", level: 1, text: "Audit report",
#     bounds: {left: 72.0, top: 61.4, right: 290.1, bottom: 85.0}}
```

`Imprimatur.images` hands over every image as a file: a PDF's JPEG and JPEG
2000 as stored, fax scans as TIFF, raw pixels as PNG (soft masks as alpha),
with the page and place each is drawn at; a Word or Excel document's media,
with the sheet that shows each. A scanned PDF gives its pages, ready for
OCR.

```ruby
Imprimatur.images("scan.pdf") do |image|
  File.binwrite("page-#{image.page}.#{image.format}", image.data)
end
```

## Command line

The same reader as a static binary, for scripts and agents that would
otherwise reach for Python to read a PDF:

```sh
cargo install imprimatur-cli
imprimatur md audit.pdf sales.xlsx         # Markdown on stdout
imprimatur --json md sales.xlsx            # the structure: typed cells, blocks, positions
imprimatur grep -i 'gots' certs/*.pdf      # matches by page, cell or section
imprimatur images -o figures/ report.pdf   # every image, as a file
```

See [`ext/imprimatur_native/cli`](ext/imprimatur_native/cli/README.md).

## Core library

The core works without Ruby too; see
[`ext/imprimatur_native/core`](ext/imprimatur_native/core/README.md).

## Development

```sh
bundle install
rake test:all        # cargo test, then the Ruby suite on the compiled extension
rake lint            # dictator, cargo fmt --check, clippy
rake gems:native     # the precompiled gem for this machine, in pkg/
```

Releases come from release-please: merging its release PR tags `vX.Y.Z`,
bumps the gem and the crates together, and the release workflow attaches the
source gem and every platform gem to the GitHub release. Publishing is local:

```sh
rake gems:pull_all   # the release's gems into pkg/, every platform checked
rake gems:push_all   # to rubygems.org, skipping platforms already published
```

## License

BSD 3-Clause. The PDF font metrics in `pdf/metrics.rs` are derived from Adobe's Core 14
AFM files, under the notice reproduced in that file.
