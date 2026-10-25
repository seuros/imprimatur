# frozen_string_literal: true

module Imprimatur
  # A flowing document (headings, paragraphs, tables, pictures, barcodes, page breaks)
  # that renders to PDF and DOCX from the same content.
  #
  #   doc = Imprimatur::Document.new(page_numbers: true)
  #   doc.heading 'Quarterly report'
  #   doc.paragraph do |p|
  #     p.text 'Revenue was '
  #     p.text 'up 12%', bold: true, color: '#1F7A1F'
  #     p.text ' this quarter.'
  #   end
  #   doc.table [%w[Region Q1], ['North', 12.5]], header: true
  #   doc.save_pdf('report.pdf')
  #   doc.save_docx('report.docx')
  #
  # PDF uses the standard Helvetica fonts for now: Windows-1252 (Latin) text
  # only, anything else raises {Imprimatur::Error}. DOCX carries any script.
  class Document
    # @param page_size [:a4, :letter, Array(Numeric, Numeric)] or its width
    #   and height in points, 36 to 1584 a side
    # @param margin [Numeric, nil] points on every side, one inch (72) unless set
    # @param font_size [Numeric, nil] body text in points, 1 to 1638 in steps of 0.5; 11 unless set
    # @param page_numbers [Boolean] number pages "1 / N" in the footer
    # @raise [ArgumentError] an unknown page size
    # @raise [Imprimatur::Error] a page size, margin or font size out of range
    def initialize(page_size: :a4, margin: nil, font_size: nil, page_numbers: false)
      @native = Native::Document.new
      numbers = page_numbers ? true : false
      if page_size.is_a?(Array)
        width, height = page_size
        @native.set_page_points(Float(width), Float(height), numbers)
      else
        @native.set_page(page_size.to_s, numbers)
      end
      @native.set_margin(Float(margin)) if margin
      @native.set_font_size(Float(font_size)) if font_size
    end

    # A PNG or JPEG in the flow, at +width+ or +height+ points (the other
    # follows its proportions), or else at 96 pixels an inch; never larger
    # than the page leaves room for.
    #
    # @param source [String, Pathname, #read] a path, the bytes or an IO
    # @param align [:left, :center, :right, nil]
    # @return [self]
    # @raise [Imprimatur::Error] no PNG or JPEG, or one it cannot draw
    def image(source, width: nil, height: nil, align: nil)
      @native.picture(Reader.read(source), width && Float(width), height && Float(height), align&.to_s)
      self
    end

    # A barcode in the flow: vector shapes in a PDF, a 300 dpi picture in DOCX.
    # Drawn at +width+ or +height+ points (a QR Code or Data Matrix keeps
    # square modules; a linear code's height is its bars'), or else at its
    # symbology's usual module size; never larger than the page leaves room for.
    # Its quiet zone is part of its size.
    #
    # @param barcode [Barcode] from {Imprimatur.barcode}
    # @param align [:left, :center, :right, nil]
    # @param color [String, nil] the bars, "#RRGGBB"; black unless set
    # @return [self]
    # @raise [Imprimatur::Error] a size out of range, or an invalid color
    def barcode(barcode, width: nil, height: nil, align: nil, color: nil)
      @native.barcode(barcode, [width && Float(width), height && Float(height)], align&.to_s, color&.to_s)
      self
    end

    # @param text [String]
    # @param level [Integer] 1 (the title) to 3
    # @return [self]
    # @raise [Imprimatur::Error] a level out of range, or a control character
    def heading(text, level: 1)
      @native.heading(level, String(text))
      self
    end

    # A paragraph, from plain text or a block of runs. +\n+ breaks the line,
    # +\t+ is a tab.
    #
    # @param text [String, nil] plain text, styled by the keywords below
    # @param align [:left, :center, :right, nil]
    # @param direction [:ltr, :rtl, nil] nil takes the direction of the first letter;
    #   right to left aligns right unless told otherwise
    # @yieldparam paragraph [Paragraph] add runs with {Paragraph#text}
    # @return [self]
    def paragraph(text = nil, align: nil, direction: nil, **look)
      paragraph = Paragraph.new
      paragraph.text(text, **look) if text
      yield paragraph if block_given?
      @native.paragraph(paragraph.runs, align&.to_s, direction&.to_s)
      self
    end

    # @param rows [Array<Array>] cells are converted with +to_s+; nil is empty
    # @param header [Boolean] the first row is a header: bold, shaded, repeated on every page
    # @return [self]
    # @raise [Imprimatur::Error] no rows or no columns
    def table(rows, header: false)
      @native.table(rows.map { |row| row.map { it.nil? ? '' : it.to_s } }, header ? true : false)
      self
    end

    # @return [self]
    def page_break
      @native.page_break
      self
    end

    # @return [String] the PDF bytes (binary)
    # @raise [Imprimatur::Error] text the standard fonts cannot encode, or a table row taller than a page
    def to_pdf = @native.render('pdf')

    # @return [String] the DOCX bytes (binary)
    def to_docx = @native.render('docx')

    # @param path [String, Pathname]
    # @return [self]
    def save_pdf(path)
      @native.save('pdf', File.expand_path(path))
      self
    end

    # @param path [String, Pathname]
    # @return [self]
    def save_docx(path)
      @native.save('docx', File.expand_path(path))
      self
    end

    # @api private
    def self.build(format, path, **options)
      document = new(**options)
      yield document
      return document.public_send(:"to_#{format}") unless path

      document.public_send(:"save_#{format}", path)
      path
    end
  end

  # Runs of text that make up one {Document#paragraph}.
  class Paragraph
    # @api private
    attr_reader :runs

    def initialize
      @runs = []
    end

    # @param text [String]
    # @param bold [Boolean]
    # @param italic [Boolean]
    # @param underline [Boolean]
    # @param color [String, nil] "#RRGGBB"
    # @return [self]
    def text(text, bold: false, italic: false, underline: false, color: nil)
      @runs << [String(text), bold ? true : false, italic ? true : false, underline ? true : false, color&.to_s]
      self
    end

    # @return [self]
    def bold(text) = text(text, bold: true)

    # @return [self]
    def italic(text) = text(text, italic: true)
  end
end
