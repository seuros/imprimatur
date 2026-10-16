# frozen_string_literal: true

module Imprimatur
  # Excel workbooks (+.xlsx+).
  #
  #   header = Imprimatur.style(bold: true, fill: '#1F4E79', color: '#FFFFFF')
  #   money = Imprimatur.style(number_format: '#,##0.00')
  #
  #   Imprimatur.xlsx('report.xlsx') do |book|
  #     book.sheet('Sales') do |sheet|
  #       sheet.row 'Region', 'Closed', 'Amount', style: header
  #       sheet.row 'North', Date.today, 12.5
  #       sheet.write(2, 2, Imprimatur.formula('SUM(C2:C2)'), style: money)
  #       sheet.column_widths 18, 12, 14
  #     end
  #   end
  #
  # Cells take String, Integer (up to 2**53), Float, true, false, {Formula},
  # Time, Date and DateTime (their wall-clock fields; no time zone); +nil+
  # leaves a cell empty.
  module Xlsx
    # Builds a workbook. With a path, saves it there and returns the path;
    # without, returns the +.xlsx+ bytes as a binary String.
    #
    # @yieldparam book [Workbook]
    # @raise [Imprimatur::Error] the workbook has no sheets, or the file cannot be written
    def self.build(path = nil)
      book = Workbook.new
      yield book
      return book.render unless path

      book.save(path)
      path
    end

    # An ordered set of uniquely named sheets.
    class Workbook
      def initialize
        @native = Native::Workbook.new
        @sheets = []
      end

      # @return [Array<Sheet>]
      attr_reader :sheets

      # Adds a sheet after the existing ones.
      #
      # @param name [String] up to 31 characters, unique ignoring case
      # @yieldparam sheet [Sheet]
      # @return [Sheet]
      # @raise [Imprimatur::Error] the name is invalid or taken
      def sheet(name)
        sheet = Sheet.new(@native, @native.add_sheet(String(name)), name)
        @sheets << sheet
        yield sheet if block_given?
        sheet
      end

      # @return [String] the +.xlsx+ bytes (binary)
      def render = @native.render

      # Writes the workbook to +path+, replacing any file there.
      #
      # @param path [String, Pathname]
      def save(path)
        @native.save(File.expand_path(path))
        self
      end
    end

    # One worksheet. Rows and columns are zero-based.
    class Sheet
      # @return [String]
      attr_reader :name

      # @api private
      def initialize(native, index, name)
        @native = native
        @index = index
        @name = name
      end

      # Appends a row below the last row written, from column A.
      #
      # @param cells [Array] values; +nil+ leaves a cell empty
      # @param style [Style, Array<Style, nil>, nil] applied to every cell
      #   written, or one per column from A; columns past the Array's end
      #   are left unstyled
      # @return [self]
      # @raise [TypeError] a value of an unsupported type
      # @raise [RangeError] an Integer beyond 2**53
      # @raise [ArgumentError] more styles than cells
      # @raise [Imprimatur::Error] the row is invalid; nothing is written
      def row(*cells, style: nil)
        @native.append_row(@index, cells, style)
        self
      end

      # Appends an Array as a row: +sheet << ['North', 12.5]+.
      #
      # @param cells [Array]
      # @return [self]
      def <<(cells) = row(*cells)

      # Sets one cell, replacing what was there.
      #
      # @param row [Integer] zero-based
      # @param column [Integer] zero-based
      # @param style [Style, nil]
      # @return [self]
      def write(row, column, value, style: nil)
        @native.write([@index, row, column], value, style)
        self
      end

      # Sets one column's width, in characters of the default font (0 to 255).
      #
      # @param column [Integer] zero-based
      # @param width [Numeric]
      # @return [self]
      def column_width(column, width)
        @native.set_column_width(@index, column, Float(width))
        self
      end

      # Sets widths from column A on; +nil+ skips a column.
      #
      # @param widths [Array<Numeric, nil>]
      # @return [self]
      def column_widths(*widths)
        widths.each_with_index { |width, column| column_width(column, width) if width }
        self
      end

      # Applies a data validation to every cell of a range, written or not:
      #
      #   sheet.validate 'G2:G1000', list: %w[Yes No]
      #   sheet.validate 'F2:F1000', list: 'Connections!$A$2:$A$1000', error: 'Pick a connection.'
      #   sheet.validate 'B2:B1000', units  # a Validation built once, shared
      #
      # @param range [String] an A1 range, "B2:B100", or a cell, "B2"
      # @param validation [Validation, nil] a rule built with {Imprimatur.validation}
      # @param options [Hash] or the options to build one, see {Validation.build}
      # @return [self]
      # @raise [Imprimatur::Error] an invalid range, or one overlapping a range
      #   already validated (Excel applies a single rule per cell)
      def validate(range, validation = nil, **options)
        raise ArgumentError, 'pass a Validation or its options, not both' if validation && options.any?

        validation ||= Validation.build(**options)
        @native.add_validation(@index, String(range), validation)
        self
      end

      # Attaches a comment (an Excel note) to a cell, replacing any it had.
      # The cell need not hold a value.
      #
      # @param reference [String] an A1 reference: "C1"
      # @param text [String] up to 32,767 characters; newlines are kept
      # @param author [String, nil]
      # @param visible [Boolean] shown all the time, not only on hover
      # @return [self]
      # @raise [Imprimatur::Error] an invalid reference or text too long
      def comment(reference, text, author: nil, visible: false)
        @native.set_comment([@index, String(reference)], String(text), author&.to_s, visible ? true : false)
        self
      end

      # Lays the sheet out right to left, column A on the right, as Excel
      # shows sheets in right-to-left languages: Hebrew, Arabic, Persian,
      # Urdu and the rest.
      #
      # @return [self]
      def right_to_left!(enabled = true)
        @native.set_right_to_left(@index, enabled ? true : false)
        self
      end

      # @return [Integer] the zero-based row the next {#row} writes
      def next_row = @native.next_row(@index)
    end
  end
end
