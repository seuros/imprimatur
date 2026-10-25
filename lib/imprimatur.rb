# frozen_string_literal: true

require_relative 'imprimatur/version'
require_relative 'imprimatur/error'
require_relative 'imprimatur/formula'
require_relative 'imprimatur/native'
require_relative 'imprimatur/style'
require_relative 'imprimatur/validation'
require_relative 'imprimatur/barcode'
require_relative 'imprimatur/xlsx'
require_relative 'imprimatur/document'
require_relative 'imprimatur/reader'
require_relative 'imprimatur/pdf_reader'
require_relative 'imprimatur/spreadsheet'
require_relative 'imprimatur/zip'

# Document authoring with a native core: XLSX workbooks, and PDF and DOCX from
# one document model.
#
#   Imprimatur.xlsx('report.xlsx') do |book|
#     book.sheet('Sales') { it.row 'Region', 'Q1' }
#   end
#
#   Imprimatur.pdf('report.pdf', page_numbers: true) do |doc|
#     doc.heading 'Report'
#     doc.paragraph 'It was printed.'
#   end
module Imprimatur
  class << self
    # @see Xlsx.build
    def xlsx(path = nil, **, &) = Xlsx.build(path, **, &)

    # @param expression [String] such as +"SUM(A1:A3)"+ or +"=A1*2"+
    # @return [Formula]
    def formula(expression) = Formula.new(expression:)

    # @see Style.build
    def style(**) = Style.build(**)

    # @see Validation.build
    def validation(**) = Validation.build(**)

    # @see Barcode.encode
    def barcode(symbology, data, **) = Barcode.encode(symbology, data, **)

    # Builds a {Document} and renders it as PDF. With a path, saves it there
    # and returns the path; without, returns the bytes.
    #
    # @param options [Hash] +page_size:+ and +page_numbers:+, see {Document#initialize}
    # @yieldparam document [Document]
    def pdf(path = nil, **options, &) = Document.build(:pdf, path, **options, &)

    # As {.pdf}, rendered as DOCX.
    def docx(path = nil, **options, &) = Document.build(:docx, path, **options, &)

    # A PDF, workbook (XLSX, XLS, ODS) or DOCX as Markdown: a PDF's text layer (headings by font
    # size, running headers and footers left out, pages marked with
    # +<!-- page N -->+), each sheet of a workbook as a table, a Word
    # document's headings, lists, emphasis, links and tables. The format is
    # told by the content, not the name.
    #
    # Reading runs without the GVL: other threads keep running, and
    # +Thread#raise+ or +Timeout+ cancels it.
    #
    #   Imprimatur.markdown('audit.pdf')
    #   Imprimatur.markdown(upload.download)        # bytes
    #   Imprimatur.markdown(File.open('sales.xlsx'))
    #
    # @param document [String, Pathname, #read] a path, the document's bytes
    #   (a binary String always is; another String only with a document's
    #   signature), or an IO. A path must name a regular file.
    # @param timeout [Numeric, nil] seconds the read may take (60), +nil+ for none
    # @param max_bytes [Integer, nil] the largest document accepted (512 MiB)
    # @param max_inflated [Integer, nil] decompressed bytes allowed in all (1 GiB)
    # @return [String] Markdown, UTF-8
    # @raise [NoTextLayer] a PDF of scanned pages: run OCR instead
    # @raise [PasswordRequired] a PDF that needs a password to open
    # @raise [UnsupportedDocument] not a PDF, XLSX, XLS, ODS or DOCX
    # @raise [LimitExceeded] the document is too large, too deep or too slow
    # @raise [Error] a damaged document
    def markdown(document, ...) = Reader.markdown(document, ...)

    # What {.markdown} renders, as data: the same structure as
    # +imprimatur --json md+ prints. A PDF's +:pages+ of +:blocks+
    # (+heading+, +paragraph+) with their +:bounds+ in points from the
    # page's top-left corner; a workbook's +:sheets+ of +:rows+ of typed
    # +:cells+ (+number+, +date+, +bool+, +text+, +error+, +formula+) and
    # +:comments+, and from an +.xlsx+ its +:columns+ widths and data
    # +:validations+; a Word document's +:blocks+ (+heading+, +paragraph+,
    # +list_item+, +table+), their bold, italic and linked +:marks+ as byte
    # ranges. Dates are ISO 8601 strings: Excel's have no time zone.
    #
    #   sheet = Imprimatur.content('sales.xlsx')[:sheets].first
    #   sheet[:rows][1][:cells]  # => [{cell: "A2", type: "text", value: "North"}, ...]
    #
    # Unlike {.markdown}, a PDF without text still returns every page, with
    # empty +:blocks+ and its image count, rather than raising {NoTextLayer}.
    # This keeps scanned and blank pages visible to OCR callers.
    #
    # Takes the arguments of {.markdown}, and raises as it does otherwise. With
    # +styles: true+ each sheet also lists its formatted cells' +:styles+:
    # font, size, bold, italic, underline, ARGB color and fill, alignment,
    # wrap, border and number format.
    #
    # @return [Hash] with symbol keys; +:format+ is +"pdf"+, +"xlsx"+ or +"docx"+
    def content(document, ...) = Reader.content(document, ...)

    # PDF metadata without reading page content.
    # @return [Hash] +page_count:+ and +encrypted:+
    def pdf_info(document, ...) = PdfReader.info(document, ...)

    # A bounded, one-based page selection. Each page has its status, text,
    # image count (nil when unknown), truncation flag and failure reason.
    # Takes +first_page:+, +last_page:+, +max_pages:+ (500),
    # +max_page_bytes:+ (256 KiB), +max_output_bytes:+ (8 MiB), and read limits.
    # @return [Hash] +page_count:+, +encrypted:+ and +pages:+
    def pdf_pages(document, ...) = PdfReader.pages(document, ...)

    # A full page as an opaque PNG, not just its embedded images.
    # Takes +page:+ (one-based), +max_side:+ (1568), +max_pixels:+ (4 million),
    # +max_output_bytes:+ (8 MiB), +grayscale:+ (true), and read limits.
    # Rendering checks time/cancellation before and after Hayro, not during it.
    # Untrusted documents still need an externally limited worker process.
    # @return [String] PNG bytes
    def render_pdf_page(document, ...) = PdfReader.render(document, ...)

    # A workbook's sheets and their values, read from the same documents
    # {.markdown} takes (+.xlsx+, +.xls+, +.ods+); see {Spreadsheet}.
    #
    # @return [Spreadsheet]
    # @raise [UnsupportedDocument] a PDF, a Word document or another format
    def spreadsheet(document, ...) = Spreadsheet.open(document, ...)

    # A document's images, as files: a PDF's, once each, from the first page
    # that draws them (JPEG and JPEG 2000 as stored, fax scans as TIFF, raw
    # pixels as PNG); a Word or Excel document's media. A scanned PDF gives
    # its pages' images, ready for OCR, instead of raising {NoTextLayer}.
    #
    #   Imprimatur.images('report.pdf').each do |image|
    #     File.binwrite("p#{image.page}.#{image.format}", image.data)
    #   end
    #
    # Takes the arguments of {.markdown}, and raises as it does (but for
    # {NoTextLayer}).
    #
    # @yieldparam image [Image] each image, when a block is given
    # @return [Array<Image>]
    def images(document, ...)
      found = Reader.images(document, ...)
      block_given? ? found.each { yield it } : found
    end
  end
end
