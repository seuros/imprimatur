# frozen_string_literal: true

require_relative 'test_helper'

class ValidationTest < Minitest::Test
  def template(&)
    ZipReader.entries(Imprimatur.xlsx { |book| book.sheet('Template', &) && book.sheet('Units') })
  end

  def test_validates_with_lists_and_formulas
    yes_no = Imprimatur.validation(list: %w[Yes No], alert: :off)
    sheet = template do |s|
      s.validate 'F2:F1000', list: '=Connections!$A$2:$A$1000', error_title: 'Invalid Connection',
                             error: 'Please select a connection from the dropdown list.'
      s.validate 'G2:G1000', yes_no
      s.validate 'H2', yes_no
      s.validate 'A2:A100', formula: 'LEN(A2)<=100', alert: :warning, prompt_title: 'Name', prompt: 'Short'
    end['xl/worksheets/sheet1.xml']

    assert_includes sheet, '<dataValidations count="4">'
    assert_includes sheet, 'errorTitle="Invalid Connection" ' \
                           'error="Please select a connection from the dropdown list." sqref="F2:F1000">' \
                           '<formula1>Connections!$A$2:$A$1000</formula1>'
    assert_includes sheet, '<dataValidation type="list" allowBlank="1" sqref="H2"><formula1>"Yes,No"</formula1>'
    assert_includes sheet, '<dataValidation type="custom" errorStyle="warning" allowBlank="1" ' \
                           'showInputMessage="1" showErrorMessage="1" promptTitle="Name" prompt="Short"'
  end

  def test_rejects_rules_excel_would
    a = Imprimatur.validation(list: %w[a])
    errors = {
      'both list and formula' => [ArgumentError, -> { Imprimatur.validation(list: %w[a], formula: 'TRUE') }],
      'neither' => [ArgumentError, -> { Imprimatur.validation(alert: :stop) }],
      'unknown alert' => [ArgumentError, -> { Imprimatur.validation(list: %w[a], alert: :loud) }],
      'comma in a value' => [Imprimatur::Error, -> { Imprimatur.validation(list: ['a,b']) }],
      'list over 255' => [Imprimatur::Error, -> { Imprimatur.validation(list: ['x' * 256]) }],
      'long title' => [Imprimatur::Error, -> { Imprimatur.validation(list: %w[a], error_title: 'x' * 33) }],
      'bad range' => [Imprimatur::Error, -> { template { it.validate('Sheet1!A1', list: %w[a]) } }],
      'overlap' => [Imprimatur::Error, -> { template { it.validate('B2:B9', a).validate('A5:C5', a) } }],
      'both forms' => [ArgumentError, -> { template { it.validate('B2', a, alert: :off) } }]
    }
    errors.each do |label, (error, build)|
      assert_raises(error, label) { build.call }
    end
  end

  def test_comments_cells
    parts = template do |s|
      s.row 'Name', 'Unit'
      s.comment 'A1', "Field: Name\nRequired: Yes", author: 'Instructions'
      s.comment 'b1', 'Pick from the list', visible: true
    end

    assert_includes parts.keys, 'xl/drawings/vmlDrawing1.vml'
    assert_includes parts['xl/comments1.xml'], '<author>Instructions</author>'
    assert_includes parts['xl/comments1.xml'], %(<comment ref="A1" authorId="0"><text><t>Field: Name\nRequired: Yes</t>)
    assert_includes parts['xl/comments1.xml'], '<comment ref="B1" authorId="1">'
    assert_includes parts['xl/drawings/vmlDrawing1.vml'], '<x:Visible/>'
    assert_includes parts['xl/worksheets/sheet1.xml'], '<legacyDrawing r:id="rId2"/>'
    assert_raises(Imprimatur::Error) { template { it.comment('A0', 'x') } }
    assert_raises(Imprimatur::Error) { template { it.comment('A1', 'x' * 32_768) } }
  end
end
