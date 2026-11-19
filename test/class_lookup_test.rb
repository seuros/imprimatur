# frozen_string_literal: true

require_relative 'test_helper'
require 'date'

# The binding resolves its classes at load: nothing it does later looks a
# constant up by name.
class ClassLookupTest < Minitest::Test
  def build
    formula = Imprimatur.formula('SUM(A1:A2)')
    bold = Imprimatur.style(bold: true)
    ZipReader.entries(Imprimatur.xlsx do |book|
      book.sheet('S') { |sheet| sheet.row 1, formula, Date.new(2026, 1, 2), style: bold }
    end).fetch('xl/worksheets/sheet1.xml')
  end

  def test_builds_after_compaction
    skip 'no GC.compact here' unless GC.respond_to?(:compact)
    expected = build
    GC.compact

    assert_equal expected, build
  end

  def test_a_redefined_constant_changes_nothing
    formula = Imprimatur.formula('A1')
    original = Imprimatur::Formula
    Imprimatur.send(:remove_const, :Formula)
    Imprimatur.const_set(:Formula, Class.new)
    sheet = ZipReader.entries(Imprimatur.xlsx { |book| book.sheet('S') { it.row formula } })

    assert_includes sheet.fetch('xl/worksheets/sheet1.xml'), '<f>A1</f>'
  ensure
    Imprimatur.send(:remove_const, :Formula)
    Imprimatur.const_set(:Formula, original)
  end

  def test_errors_keep_their_class_when_the_constant_is_gone
    original = Imprimatur::LimitExceeded
    Imprimatur.send(:remove_const, :LimitExceeded)

    assert_raises(original) { Imprimatur.markdown('%PDF-1.7', max_bytes: 1) }
  ensure
    Imprimatur.const_set(:LimitExceeded, original)
  end

  def test_no_constant_lookup_runs_ruby_code
    expected = build
    Object.define_singleton_method(:const_get) { |*| raise 'const_get was called' }

    assert_equal expected, build
  ensure
    Object.singleton_class.send(:remove_method, :const_get)
  end
end
