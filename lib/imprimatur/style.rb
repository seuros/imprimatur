# frozen_string_literal: true

module Imprimatur
  # How a cell looks. Built once with {Imprimatur.style}, validated then, and
  # shared by every cell that uses it. The class itself is native.
  class Style
    # @param number_format [String, nil] an Excel format code: "0.00", "#,##0", "0%", "yyyy-mm-dd"
    # @param bold [Boolean]
    # @param italic [Boolean]
    # @param underline [Boolean]
    # @param color [String, nil] font color, "#RRGGBB"
    # @param fill [String, nil] background color, "#RRGGBB"
    # @param align [:left, :center, :right, nil]
    # @param direction [:ltr, :rtl, nil] reading order; without one Excel infers it from the text
    # @param wrap [Boolean] wrap text within the column width
    # @param border [:thin, :medium, :thick, nil] on all four sides
    # @param font [String, nil] typeface, Calibri by default: "Arial"
    # @param size [Numeric, nil] in points, 1 to 409 in steps of 0.5; 11 by default
    # @param valign [:top, :center, :bottom, nil] vertical alignment; Excel's default is the bottom
    # @return [Style]
    # @raise [Imprimatur::Error] an invalid number format, color, font or size
    # @raise [ArgumentError] an unknown align, direction, border or valign
    def self.build(number_format: nil, bold: false, italic: false, underline: false, color: nil, fill: nil,
                   align: nil, direction: nil, wrap: false, border: nil, font: nil, size: nil, valign: nil)
      __build(number_format&.to_s, bold, italic, underline, color&.to_s, fill&.to_s,
              align&.to_s, direction&.to_s, wrap, border&.to_s, font&.to_s, size && Float(size), valign&.to_s)
    end

    private_class_method :__build
  end
end
