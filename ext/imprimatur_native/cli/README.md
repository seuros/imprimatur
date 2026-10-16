# imprimatur-cli

`imprimatur`: PDF, XLSX and DOCX documents from the command line: as
Markdown or JSON, searched, or their images. The reader is the
[imprimatur](../core/README.md) crate's: no Python, no Poppler, one static
binary.

```sh
cargo install imprimatur-cli

imprimatur markdown audit.pdf                 # Markdown on stdout
imprimatur md -o notes/ *.docx *.xlsx         # notes/NAME.md for each
curl -s "$URL" | imprimatur md -              # standard input
imprimatur --json md sales.xlsx               # the structure, as JSON

imprimatur grep -i 'non.?conformity' audits/*.pdf
imprimatur grep -F 'ISO 17065' -C 1 cert.pdf
imprimatur images -o figures/ report.pdf      # figures/report-p3-1.png...
```

## JSON

`--json` prints one object per document, on its own line. For `md`, that is
the document's structure, not its Markdown:

- PDF: `pages` of `blocks` (`heading` with its `level`, `paragraph`), each
  with its `bounds` in points from the page's top-left corner, and how many
  `images` the page draws.
- XLSX: `sheets` of `rows` of `cells`, each typed: `number`, `date`,
  `bool`, `text`, `error` or `formula`, its `value` and, when they differ,
  its `text` as Excel shows it; then the sheet's `comments`.
- DOCX: `blocks`: `heading`, `paragraph`, `list_item`, `table`; text keeps
  `marks`, byte ranges that are bold, italic or a link.

## grep

Searches what `md` reads, line by line, and says where each match is: `p3`
(page 3), `Sales!B2` (a cell), `Sales!B2 (comment)`, `§Scope` (the heading
a Word paragraph falls under). `-i`, `-F`, `-w`, `-C N`, `-m N`, `-l`, `-c`
and `-q` work as grep's do, and so does the exit status: 0 when a line
matched, 1 when none did, 2 on trouble (a scanned PDF, a damaged file, a
wrong pattern). With `--json`, each match carries the byte offsets of what
matched.

## images

Writes every image into `-o DIR`, once each, from the first page that draws
it: JPEG and JPEG 2000 as stored, fax scans as TIFF, raw pixels as PNG (CMYK
converted, soft masks as alpha). A scanned PDF gives its page images, ready
for OCR. An Office document's media are copied, with the sheet that shows
each. `--min-size PX` skips icons and rules; `--json` adds each image's
format, size and place on its page.

## Exit status

The format is told by the content, not the name. Reads are bounded:
`--timeout` (60 seconds), `--max-bytes` (512 MiB), `--max-inflated` (1 GiB
of decompressed data), so a hostile document fails fast.

For `md` and `images`, a document that fails does not stop the others; the
exit status is the first failure's, so scripts can branch on it:

| Status | Meaning |
|---|---|
| 0 | every document was read |
| 1 | a document is damaged, or a file could not be read or written |
| 2 | the command line is wrong |
| 3 | a PDF has no text layer: its pages are scans, which need OCR |
| 4 | a PDF is encrypted with a password |
| 5 | not a PDF, XLSX or DOCX |
| 6 | a limit was hit: size, decompressed size, nesting or time |

With `--json`, failures are reported in the JSON too (`"ok": false`, `"error":
"no_text_layer"`...), and nothing is written to standard error.

Shell completions: `imprimatur completion zsh` (bash, fish, elvish, nu and
powershell too). The full command-line spec: `imprimatur __usage_spec__`.

## License

BSD 3-Clause
