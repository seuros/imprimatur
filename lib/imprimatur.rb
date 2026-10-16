# frozen_string_literal: true

require_relative 'imprimatur/version'
require_relative 'imprimatur/error'
require_relative 'imprimatur/formula'
require_relative 'imprimatur/native'
require_relative 'imprimatur/style'
require_relative 'imprimatur/validation'
require_relative 'imprimatur/xlsx'
require_relative 'imprimatur/document'
require_relative 'imprimatur/reader'

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
    def xlsx(path = nil, &) = Xlsx.build(path, &)

    # @param expression [String] such as +"SUM(A1:A3)"+ or +"=A1*2"+
    # @return [Formula]
    def formula(expression) = Formula.new(expression:)

    # @see Style.build
    def style(**) = Style.build(**)

    # @see Validation.build
    def validation(**) = Validation.build(**)

    # Builds a {Document} and renders it as PDF. With a path, saves it there
    # and returns the path; without, returns the bytes.
    #
    # @param options [Hash] +page_size:+ and +page_numbers:+, see {Document#initialize}
    # @yieldparam document [Document]
    def pdf(path = nil, **options, &) = Document.build(:pdf, path, **options, &)

    # As {.pdf}, rendered as DOCX.
    def docx(path = nil, **options, &) = Document.build(:docx, path, **options, &)

    # A PDF, XLSX or DOCX as Markdown: a PDF's text layer (headings by font
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
    #   (told apart by their signature), or an IO
    # @param timeout [Numeric, nil] seconds the read may take (60), +nil+ for none
    # @param max_bytes [Integer, nil] the largest document accepted (512 MiB)
    # @param max_inflated [Integer, nil] decompressed bytes allowed in all (1 GiB)
    # @return [String] Markdown, UTF-8
    # @raise [NoTextLayer] a PDF of scanned pages: run OCR instead
    # @raise [PasswordRequired] a PDF that needs a password to open
    # @raise [UnsupportedDocument] not a PDF, XLSX or DOCX
    # @raise [LimitExceeded] the document is too large, too deep or too slow
    # @raise [Error] a damaged document
    def markdown(document, ...) = Reader.markdown(document, ...)

    # What {.markdown} renders, as data: the same structure as
    # +imprimatur --json md+ prints. A PDF's +:pages+ of +:blocks+
    # (+heading+, +paragraph+) with their +:bounds+ in points from the
    # page's top-left corner; a workbook's +:sheets+ of +:rows+ of typed
    # +:cells+ (+number+, +date+, +bool+, +text+, +error+, +formula+) and
    # +:comments+; a Word document's +:blocks+ (+heading+, +paragraph+,
    # +list_item+, +table+), their bold, italic and linked +:marks+ as byte
    # ranges. Dates are ISO 8601 strings: Excel's have no time zone.
    #
    #   sheet = Imprimatur.content('sales.xlsx')[:sheets].first
    #   sheet[:rows][1][:cells]  # => [{cell: "A2", type: "text", value: "North"}, ...]
    #
    # Takes the arguments of {.markdown}, and raises as it does.
    #
    # @return [Hash] with symbol keys; +:format+ is +"pdf"+, +"xlsx"+ or +"docx"+
    def content(document, ...) = Reader.content(document, ...)

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
