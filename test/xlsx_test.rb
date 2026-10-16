# frozen_string_literal: true

require_relative 'test_helper'
require 'bigdecimal'

class XlsxTest < Minitest::Test
  def sales
    Imprimatur.xlsx do |book|
      book.sheet('Sales') do |sheet|
        sheet.row 'Region', 'Q1', 'Done'
        sheet.row 'North & West', 12.5, true
        sheet << ['South', 7, false]
        sheet.row 'Total', Imprimatur.formula('=SUM(B2:B3)')
      end
    end
  end

  def test_renders_a_binary_package
    bytes = sales
    parts = ZipReader.entries(bytes)

    assert_equal Encoding::BINARY, bytes.encoding
    assert_includes parts.keys, 'xl/worksheets/sheet1.xml'
    assert_includes parts['xl/workbook.xml'], '<sheet name="Sales" sheetId="1" r:id="rId1"/>'
    assert_equal sales, bytes, 'output is deterministic'
  end

  def test_writes_each_kind_of_cell
    parts = ZipReader.entries(sales)
    sheet = parts['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<c r="B2"><v>12.5</v></c><c r="C2" t="b"><v>1</v></c>'
    assert_includes sheet, '<c r="B3"><v>7</v></c><c r="C3" t="b"><v>0</v></c>'
    assert_includes sheet, '<c r="B4"><f>SUM(B2:B3)</f></c>'
    assert_includes parts['xl/sharedStrings.xml'], '<si><t>North &amp; West</t></si>'
  end

  def test_nil_leaves_a_cell_empty_and_write_sets_one
    bytes = Imprimatur.xlsx do |book|
      book.sheet('Gaps') do |sheet|
        sheet.row 'a', nil, 'c'
        sheet.write(5, 27, 1.5)
        assert_equal 6, sheet.next_row
      end
    end

    sheet = ZipReader.entries(bytes)['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>1</v></c></row>'
    assert_includes sheet, '<row r="6"><c r="AB6"><v>1.5</v></c></row>'
    assert_includes sheet, '<dimension ref="A1:AB6"/>'
  end

  def test_saves_to_a_path
    Dir.mktmpdir do |dir|
      path = Pathname(dir).join('out.xlsx')
      result = Imprimatur.xlsx(path) { it.sheet('One').row(1) }

      assert_equal path, result
      assert_includes ZipReader.entries(File.binread(path)), 'xl/workbook.xml'
    end
  end

  def test_rejects_invalid_workbooks
    errors = {
      'duplicate sheet' => -> { it.sheet('A') && it.sheet('a') },
      'bad sheet name' => -> { it.sheet('a/b') },
      'NaN' => -> { it.sheet('A').row(Float::NAN) },
      'long text' => -> { it.sheet('A').row('x' * 32_768) },
      'column out of range' => -> { it.sheet('A').write(0, 16_384, 1) },
      'no sheets' => ->(_) {}
    }
    errors.each do |label, build|
      assert_raises(Imprimatur::Error, label) { Imprimatur.xlsx(&build) }
    end
  end

  # MOCKED: ActiveSupport::TimeWithZone, which claims to be a Time through
  # is_a? and answers the wall clock of its zone.
  class ZonedTime
    def initialize(time) = @time = time
    def is_a?(klass) = klass == Time || super
    %i[year month day hour min sec nsec].each { |field| define_method(field) { @time.public_send(field) } }
  end

  def test_writes_decimals_rationals_and_zoned_times
    tokyo_noon = Time.new(2023, 3, 15, 12, 0, 0, '+09:00')
    bytes = Imprimatur.xlsx do |book|
      book.sheet('Values').row BigDecimal('12.5'), Rational(3, 4), ZonedTime.new(tokyo_noon)
    end
    sheet = ZipReader.entries(bytes)['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<c r="A1"><v>12.5</v></c><c r="B1"><v>0.75</v></c>'
    assert_includes sheet, '<c r="C1" s="1"><v>45000.5</v></c>', 'the zone\'s wall clock'
    assert_raises(Imprimatur::Error) { Imprimatur.xlsx { it.sheet('A').row(BigDecimal('NaN')) } }
    assert_raises(RangeError) { Imprimatur.xlsx { it.sheet('A').row(Complex(1, 2)) } }
  end

  def test_rejects_values_it_cannot_store
    book = Imprimatur::Xlsx::Workbook.new
    sheet = book.sheet('A')

    assert_raises(TypeError) { sheet.row(:symbol) }
    assert_raises(TypeError) { sheet.row(1..2) }
    assert_raises(RangeError) { sheet.row((2**53) + 1) }
    assert_raises(RangeError) { sheet.row(2**64) }
    assert_raises(TypeError) { Imprimatur.formula(42) }
    sheet.row(2**53, -(2**53))

    assert_equal 1, sheet.next_row, 'rejected rows write nothing'
  end
end
