# frozen_string_literal: true

require_relative 'test_helper'
require 'date'

class StyleTest < Minitest::Test
  def styled_parts(&)
    ZipReader.entries(Imprimatur.xlsx { |book| book.sheet('Styled', &) })
  end

  def test_styles_rows_and_cells
    header = Imprimatur.style(bold: true, color: '#FFFFFF', fill: '#1F4E79', align: :center, border: :thin)
    money = Imprimatur.style(number_format: '#,##0.00')
    parts = styled_parts do |sheet|
      sheet.row 'Region', 'Amount', style: header
      sheet.row 'North'
      sheet.write(1, 1, 12.5, style: money)
    end
    sheet = parts['xl/worksheets/sheet1.xml']
    styles = parts['xl/styles.xml']

    assert_includes sheet, '<c r="A1" s="1" t="s">'
    assert_includes sheet, '<c r="B2" s="2"><v>12.5</v></c>'
    assert_includes styles, '<numFmt numFmtId="164" formatCode="#,##0.00"/>'
    assert_includes styles, '<fgColor rgb="FF1F4E79"/>'
    assert_includes styles, '<alignment horizontal="center"/>'
  end

  def test_fonts_sizes_and_vertical_alignment
    header = Imprimatur.style(font: 'Arial', size: 12, bold: true, valign: :center)
    parts = styled_parts do |sheet|
      sheet.row 'Name', style: header
      sheet.row 'Acme', style: Imprimatur.style(font: 'Arial', size: 10.5)
    end
    styles = parts['xl/styles.xml']

    assert_includes styles, '<font><b/><sz val="12"/><name val="Arial"/></font>'
    assert_includes styles, '<font><sz val="10.5"/><name val="Arial"/></font>'
    assert_includes styles, '<alignment vertical="center"/>'
  end

  def test_styles_cells_of_a_row_one_by_one
    fill = Imprimatur.style(fill: '#FFFF00')
    bold = Imprimatur.style(bold: true)
    sheet = styled_parts { it.row('a', 'b', 'c', 'd', style: [fill, nil, bold]) }['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<c r="A1" s="1" t="s">'
    assert_includes sheet, '<c r="B1" t="s">'
    assert_includes sheet, '<c r="C1" s="2" t="s">'
    assert_includes sheet, '<c r="D1" t="s">', 'past the styles, cells are unstyled'
    assert_raises(ArgumentError) { styled_parts { it.row('a', style: [fill, fill]) } }
    assert_raises(TypeError) { styled_parts { it.row('a', style: ['bold']) } }
  end

  def test_writes_dates_times_and_datetimes
    parts = styled_parts do |sheet|
      tokyo_noon = Time.new(2023, 3, 15, 12, 0, 0, '+09:00')
      sheet.row Date.new(2023, 3, 15), tokyo_noon, DateTime.new(2023, 3, 15, 18)
    end
    sheet = parts['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<c r="A1" s="1"><v>45000</v></c>'
    assert_includes sheet, '<c r="B1" s="2"><v>45000.5</v></c>', 'the wall clock, not UTC'
    assert_includes sheet, '<c r="C1" s="2"><v>45000.75</v></c>'
    assert_includes parts['xl/styles.xml'], 'formatCode="yyyy-mm-dd hh:mm:ss"'
  end

  def test_widths_and_right_to_left
    parts = styled_parts do |sheet|
      rtl = Imprimatur.style(direction: :rtl)
      sheet.right_to_left!.column_widths(18, nil, 12.5).row('مرحبا', 'שלום', style: rtl)
    end
    sheet = parts['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<sheetView rightToLeft="1" workbookViewId="0"/>'
    assert_includes sheet, '<col min="1" max="1" width="18" customWidth="1"/>'
    assert_includes sheet, '<col min="3" max="3" width="12.5" customWidth="1"/>'
    assert_includes parts['xl/styles.xml'], '<alignment readingOrder="2"/>'
    assert_includes parts['xl/sharedStrings.xml'], '<t>مرحبا</t>'
  end

  def test_rejects_invalid_styles_when_built
    assert_raises(Imprimatur::Error) { Imprimatur.style(color: 'red') }
    assert_raises(Imprimatur::Error) { Imprimatur.style(number_format: '') }
    assert_raises(ArgumentError) { Imprimatur.style(align: :justify) }
    assert_raises(ArgumentError) { Imprimatur.style(direction: :up) }
    assert_raises(ArgumentError) { Imprimatur.style(typeface: 'Comic Sans') }
    assert_raises(ArgumentError) { Imprimatur.style(valign: :middle) }
    assert_raises(Imprimatur::Error) { Imprimatur.style(font: 'x' * 32) }
    assert_raises(Imprimatur::Error) { Imprimatur.style(size: 10.25) }
    assert_raises(ArgumentError) { Imprimatur.style(size: 'big') }
    assert_raises(TypeError) { Imprimatur.xlsx { it.sheet('A').row(1, style: { bold: true }) } }
    assert_raises(Imprimatur::Error) { Imprimatur.xlsx { it.sheet('A').column_width(0, 300) } }
    assert_raises(Imprimatur::Error) { Imprimatur.xlsx { it.sheet('A').row(Date.new(1899, 12, 31)) } }
  end
end
