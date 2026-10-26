# imprimatur

*Imprimatur*: "let it be printed", the stamp a censor put on a book before it
could go to press. This one stamps spreadsheets, PDFs and Word documents. No heresy reaches the printer: an invalid sheet name, a NaN or
a 40,000-character cell is refused at the door, not discovered by Excel's
"We found a problem with some content".

- **A Ruby DSL over a native core.** The core is also published on its own,
  as `imprimatur` on crates.io; the gem wraps it.
- **Own readers and writers.** XML, zip and PDF reading/writing live here.
  Optional full-page PDF rendering uses Hayro, a pure-Rust CPU renderer;
  no Poppler, PDFium, GPU or system fonts are required.
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

Precompiled gems ship for Linux (x86_64 and aarch64, glibc 2.34+),
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

The workbook can have a font of its own, for unstyled cells and for styles
that name no font or size, and size its columns to their cells:

```ruby
Imprimatur.xlsx("export.xlsx", font: "Arial", size: 10, autofit: true) do |book|
  book.sheet("Orders") { it.row "Supplier", "Amount", style: header }
  book.sheet("Notes").autofit!(false)          # this one keeps Excel's widths
end
```

`autofit` (or `sheet.autofit!`) sizes each column without a width of its own
to its widest cell, estimated as `(characters + 3) × points / 10`,
bold cells one and a half times wider, at most 255. A text counts its longest
line, a date `yyyy-mm-dd`; formulas are left out, their results unknown.

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

book.sheet("مبيعات") do |sheet|                        # Arabic: right to left
  sheet.row "المنطقة", "الشمال", style: rtl              # column A on the right
end

book.sheet("Suppliers") do |sheet|
  sheet.right_to_left!                                 # told so: a Hebrew layout
  sheet.row "אזור", "Region"
end
```

Text in any right-to-left language needs nothing special: Hebrew, Yiddish,
Arabic, Persian, Urdu, Pashto, Sindhi, Kurdish (Sorani), Uyghur, Syriac,
Dhivehi (Thaana), N'Ko, Adlam and the rest. A sheet whose letters (its name
and text cells) are mostly right to left is laid out the way Excel does for
them, column A on the right; `right_to_left!` and `right_to_left!(false)`
decide instead. `direction: :rtl` fixes a cell's reading order instead of
letting Excel infer it from its first letter.

Not yet: merged cells, frozen panes, streaming for very large sheets.

## Documents: PDF and DOCX

One document, two formats:

```ruby
doc = Imprimatur::Document.new(page_size: :a4, font_size: 10, page_numbers: true)
doc.heading "Quarterly report"
doc.heading "Sales", level: 2
doc.paragraph do |p|
  p.text "Revenue was "
  p.text "up 12%", bold: true, color: "#1F7A1F"
  p.text " this quarter."
end
doc.paragraph "Figures are unaudited.", italic: true, align: :center
doc.table [["Region", "Q1"], ["North", 12_500.5]], header: true
doc.image "chart.png", width: 300, align: :center
doc.page_break
doc.paragraph "Appendix."

doc.save_pdf("report.pdf")
doc.save_docx("report.docx")

Imprimatur.pdf("memo.pdf", font_size: 10) { it.paragraph "Or in one go." } # Imprimatur.docx alike
Imprimatur.pdf(page_size: [200, 200], margin: 10) { it.image qr_png, width: 180 }
```

- Headings 1 to 3 (real Word heading styles in DOCX), paragraphs of runs
  (bold, italic, underline, color), alignment, `\n` line breaks and tabs.
- Body text at `font_size:` points (1 to 1638, in steps of 0.5; 11 unless
  set), in both formats; headings keep their own sizes.
- Tables: equal columns, borders, a shaded header row that repeats on every
  page; cells are converted with `to_s`.
- Pictures: PNG or JPEG (a path, the bytes or an IO), at `width:` or
  `height:` points (the other follows), or else at 96 pixels an inch, never
  larger than the page; `align:` like a paragraph. In PDF a JPEG and an
  opaque PNG are stored as they are, a PNG with alpha gets a soft mask.
  Interlaced PNGs are refused.
- Page breaks; A4, Letter or `page_size: [width, height]` in points (36 to
  1584); `margin:` in points (one inch unless set); `1 / N` page numbers.
- Right to left without being told: a paragraph, heading or table cell
  reads in the direction of its first letter (right aligned, and `w:bidi` in
  DOCX), figures alone in their table's or document's, each run in its own;
  a mostly right-to-left table has its first column on the right, and a
  mostly right-to-left document its pages laid out right to left.
  `direction: :rtl` or `:ltr` decides for a paragraph.

**PDF text is Latin only for now.** PDFs use the standard Helvetica fonts,
which carry Windows-1252: English and Western European text, curly quotes,
dashes, the euro sign. Anything else (Arabic, Hebrew, Greek, CJK, emoji) raises
`Imprimatur::Error` naming the character, never a `?`. DOCX carries any
script. Embedded TrueType fonts, and with them every script and proper RTL in
PDF, are the next step.

## Barcodes

QR Code, Data Matrix, EAN-13, EAN-8, UPC-A and Code 128, and the GS1 forms of
QR Code, Data Matrix and Code 128 (GS1-128):

```ruby
qr = Imprimatur.barcode(:qr, "https://id.gs1.org/01/09506000134352", level: :m)
File.write("qr.svg", qr.svg(module_size: 4))
File.binwrite("qr.png", qr.png(module_size: 8, background: nil)) # transparent

ean = Imprimatur.barcode(:ean13, "950600013435")
ean.text # => "9506000134352", the check digit added (or checked when given)

label = Imprimatur.barcode(:code128, "(00)106141412345678908", gs1: true)  # GS1-128
dpp = Imprimatur.barcode(:datamatrix, "(01)09506000134352(21)ABC123", gs1: true)

Imprimatur.pdf("label.pdf", page_size: [300, 200], margin: 10) do |doc|
  doc.barcode label, width: 280
  doc.barcode dpp, width: 72, align: :right
end
```

- `:qr` (`level:` `:l`, `:m`, `:q` or `:h`), `:datamatrix` (square),
  `:ean13`, `:ean8`, `:upca` and `:code128`; `gs1: true` for the GS1 forms of
  `:qr`, `:datamatrix` and `:code128`.
- GS1 element strings are written as people write them, AIs in parentheses,
  and checked: AI syntax, GS1's character set, the AIs whose lengths are
  predefined, and the check digits of SSCCs, GTINs and GLNs. Separators go in
  only where a variable-length field needs one.
- `svg` and `png` take `module_size:`, `color:`, `background:` (nil for
  none), `quiet_zone:` and, for linear codes, `bar_height:` in modules. PNGs
  are one bit a pixel.
- In documents, barcodes are vector shapes in a PDF and a 300 dpi picture in
  DOCX, at `width:` or `height:` points or else at their usual module size
  (0.33 mm for EAN and UPC, 0.495 mm for GS1-128, 0.7 mm for QR Code and Data
  Matrix), quiet zone included.
- `text`, `symbology`, `gs1?`, `width`, `height` (in modules), and `modules`
  (rows of booleans) for drawing them some other way.

## Reading: documents as Markdown

The other direction: a PDF, a workbook (XLSX, legacy XLS, ODS) or a DOCX
printed as Markdown, for search indexes and language models.

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
  ToUnicode maps, forms, rotated pages), in reading order, with headings by
  font size, hyphenated line breaks rejoined, presentation forms (`ﬁ`,
  Arabic contextual forms) as plain letters, and running headers and
  footers left out. Pages after the first start with `<!-- page N -->`.
  Encrypted PDFs open when the user password is empty, as it is for
  documents that only restrict printing or copying (RC4, AES-128, AES-256).
- **Right to left in PDFs** (Hebrew, Arabic, Persian, Urdu, Syriac,
  Dhivehi, N'Ko, Adlam): a PDF records where glyphs go, not the order they
  read in, so each line is put back in order the way the Unicode
  bidirectional algorithm laid it out, with its page's direction for
  context. An Arabic page's table rows read from the right, figures
  included; an Arabic or Hebrew name in an English line reads forwards
  inside that line; numbers stay whole (`2024-03-15`, `1,250.50`, `50%`,
  `+98 21 5555 1234`, Arabic-Indic and Persian digits); vowel marks stay on
  their letters. Not yet: pages set in several columns, whose lines run
  together whatever their direction.
- **XLSX**: each sheet as a table under its name, values as Excel shows them
  (dates as dates, percentages, 15 significant digits), formulas without a
  cached result as `=FORMULA`, then the sheet's comments.
- **XLS** (Excel 97–2003, and Excel 5.0/95 in Western European code pages)
  and **ODS** (LibreOffice, OpenOffice): the same, sheet by sheet; an ODS
  number keeps the text it shows. Legacy `.xls` comments are left out.
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
`.doc`, `.xlsb`, presentations) are the other errors to expect.

A binary String is always the document's bytes, never a path: an upload
holding `/etc/passwd` or `/dev/zero` is refused as `UnsupportedDocument`,
not read from the server. A path (a `Pathname`, or a String that is not
binary and has no document signature) must name a regular file; devices,
pipes and directories are refused. An IO or a path is read no further than
`max_bytes:`, so an endless stream or a huge file stops there.

### Structure and images

Markdown is one rendering of what a read finds. `Imprimatur.content` gives
the structure itself, the same as `imprimatur --json md` prints: a PDF's
pages of headings and paragraphs with their position, a workbook's typed
cells and comments, a Word document's blocks with their bold, italic and
linked stretches.

For a PDF without text, `content` still returns every page, its empty
`blocks` and its `images` count. `markdown` and the text-oriented CLI commands
still report `NoTextLayer`;
OCR callers need the structure of scanned and blank pages too.

### Bounded PDF pages and OCR rendering

```ruby
Imprimatur.pdf_info(upload.download) # => {page_count: 20, encrypted: false}
result = Imprimatur.pdf_pages(upload.download, first_page: 2, last_page: 10, max_pages: 5)
result[:pages].each { |page| puts [page[:page], page[:status], page[:text]] }
png = Imprimatur.render_pdf_page(upload.download, page: 2, max_side: 1568)
```

`pdf_info` counts actual pages without interpreting their content. `pdf_pages`
only interprets the selected streams, at most 500 pages by default. Each page
has `status` (`text`, `empty`, `failed`, `skipped`), `text`, `images` (nil if
unknown), `truncated` and `reason`. It keeps healthy pages when another is
damaged. Unlike the tolerant Markdown reader, corrupt Flate streams fail
explicitly. Limits are 256 KiB of text per page and 8 MiB total
(`max_page_bytes:`, `max_output_bytes:`); truncation respects UTF-8.

`render_pdf_page` returns an opaque grayscale PNG of the **whole page**, not
just its scan image; `grayscale: false` keeps color. It bounds the output surface
before allocation (`max_pixels:`, 4 million) and encoded output
(`max_output_bytes:`, 8 MiB). Preflight rejects pages drawing more than 40 million
source-image pixels. Renderer warnings fail instead of silently dropping content.
The crate exposes `read::pdf::{info, pages, render_page}` (`render` feature) and
`read::pdf::isolation::{info_isolated, pages_isolated, render_page_isolated}`
(`isolation` feature). These take an absolute path to the trusted
`imprimatur-render-worker` executable. The gem bundles that worker and always
isolates all three PDF operations; it never falls back to in-process parsing
or searches PATH.

All three accept the normal read limits and `memory_bytes:`
(1 GiB by default): Linux/FreeBSD enforce an address-space cap; macOS uses a
sampled RSS watchdog, which can briefly overshoot. A worker is killed and reaped
on timeout, cancellation or supervisor failure. Each operation has a mandatory
60-second process ceiling even with `timeout: nil`; a shorter `timeout:` still
applies. CPU time, output files and core dumps are also limited.

The page worker flushes completed records as it goes. An exhausted stream
budget triggers individual-page recovery inside that worker: each attempt has
its own `max_inflated` allowance, while the original overall deadline, process
memory limit and aggregate text cap remain in force. An oversized page is
`failed`, not blank, without discarding healthy neighbors. A worker terminated
after metadata was read preserves completed pages and explicitly marks the
unreached pages; an expired overall deadline marks them `skipped`. Cancellation
still raises rather than returning a partial successful call.

This is resource containment, **not a filesystem/network security sandbox**.
The raw crate `info`, `pages` and `render_page` APIs remain in-process;
`render_page` cannot interrupt Hayro.
`max_inflated` bounds our preflight, not Hayro's internal decoders.

```ruby
sheet = Imprimatur.content("sales.xlsx")[:sheets].first
sheet[:rows][1][:cells]
# => [{cell: "A2", type: "text", value: "North"},
#     {cell: "B2", type: "number", value: 12.5, text: "12.5"}]

Imprimatur.content("audit.pdf")[:pages].first[:blocks].first
# => {type: "heading", level: 1, text: "Audit report",
#     bounds: {left: 72.0, top: 61.4, right: 290.1, bottom: 85.0}}
```

An `.xlsx` sheet also gives its `columns` widths and data `validations`, and
with `styles: true` the look of each formatted cell, empty ones included:

```ruby
sheet = Imprimatur.content("template.xlsx", styles: true)[:sheets].first
sheet[:validations].first  # => {range: "G2:G1000", type: "list", formula1: "\"Yes,No\"", ...}
sheet[:styles].first       # => {cell: "A1", font: "Arial", size: 12.0, bold: true, fill: "FF808080", ...}
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

### Workbooks as rows

For importing uploads, `Imprimatur.spreadsheet` reads an `.xlsx`, `.xls` or
`.ods` into rows of Ruby values: Strings, Integers for whole numbers shown
without a decimal point or a percent sign, Floats, `true`/`false`, Dates,
DateTimes for a time of day, and `nil` for an empty cell or a formula never
computed.

```ruby
book = Imprimatur.spreadsheet(upload.download)   # a path, the bytes or an IO
book.names                                      # => ["Suppliers", "Countries"]
book.sheet.rows.first                           # => ["Instructions: one a row"]

book.sheet("Suppliers").parse(clean: true, name: "Company Name", since: "Since")
# => [{ name: "Acme", since: #<Date: 2024-03-15> }, ...]
```

`parse` finds the header row among the first 100 (`header_rows:`), the first
holding every header, and maps the rows under it. `clean: true` strips control
characters and the whitespace at both ends of every String. Without such a row
it raises `Imprimatur::HeaderRowNotFound`, whose `missing` lists the headers the
closest row lacks.

Each sheet also has its `comments`, its `columns` (runs of widths, and whether
hidden) and its `validations`, as `Imprimatur.content` gives them. Opened with
`styles: true`, it has every formatted cell's look too:

```ruby
sheet = Imprimatur.spreadsheet("template.xlsx", styles: true).sheet("BOM")
sheet.validation("G2:G1000")   # => { range: "G2:G1000", type: "list", formula1: "Lists!$A$1:$A$9", ... }
sheet.style("A1")              # => { cell: "A1", font: "Arial", bold: true, fill: "FFDDEEFF", ... }
```

## Zip archives

Archives of any files, for what is not a document (an export of an order's
files, a download to unpack). Entries keep their modification times and
comments, files are streamed from disk, and archives past 4 GiB use ZIP64;
reading is bounded per entry. Both run without the GVL.

```ruby
Imprimatur::Zip.write("export.zip") do |zip|
  zip.add_file("contracts/0contract.pdf", "/tmp/contract.pdf", comment: "Contract")
  zip.add("manifest.json", JSON.generate(manifest))
end

Imprimatur::Zip.open("geolite.zip") do |zip|
  csv = zip.read(zip.glob("**/*-Locations-en.csv").first) # max_bytes: 1 GiB by default
  zip.extract("README.txt", "/tmp/readme.txt")
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
rake test:all        # core, CLI and Ruby suites on the compiled extension
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
