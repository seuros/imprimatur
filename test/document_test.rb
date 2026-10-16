# frozen_string_literal: true

require_relative 'test_helper'

class DocumentTest < Minitest::Test
  def report
    Imprimatur::Document.new(page_numbers: true).tap do |doc|
      doc.heading 'Quarterly report'
      doc.heading 'Detail', level: 2
      doc.paragraph do |p|
        p.text 'Revenue was '
        p.text 'up 12%', bold: true, color: '#1F7A1F'
        p.italic ' (unaudited)'
      end
      doc.paragraph 'Centered (and parenthesised).', align: :center
      doc.table [['Region', 'Q1', nil], ['North', 12.5, true]], header: true
      doc.page_break
      doc.paragraph 'The end.'
    end
  end

  # The decompressed content streams of a PDF, one per page.
  def pdf_pages(bytes)
    bytes.scan(/stream\n(.*?)\nendstream/m).map { Zlib::Inflate.inflate(it.first).force_encoding(Encoding::BINARY) }
  end

  def test_renders_pdf
    bytes = report.to_pdf
    pages = pdf_pages(bytes)

    assert bytes.start_with?('%PDF-1.7')
    assert bytes.end_with?("%%EOF\n")
    assert_equal 2, pages.size
    assert_includes pages[0], '(Quarterly report) Tj'
    assert_includes pages[0], '(Centered \(and parenthesised\).) Tj'
    assert_includes pages[0], '(1 / 2) Tj'
    assert_includes pages[1], '(The end.) Tj'
    assert_equal bytes, report.to_pdf, 'deterministic'
  end

  def test_renders_docx_from_the_same_content
    parts = ZipReader.entries(report.to_docx)
    body = parts['word/document.xml']

    assert_includes body, '<w:pStyle w:val="Heading2"/>'
    assert_includes body, '<w:rPr><w:b/><w:color w:val="1F7A1F"/></w:rPr><w:t xml:space="preserve">up 12%</w:t>'
    assert_includes body, '<w:t xml:space="preserve">12.5</w:t>'
    assert_includes body, '<w:br w:type="page"/>'
    assert_includes parts.keys, 'word/footer1.xml'
  end

  def test_block_builders_save_and_return
    Dir.mktmpdir do |dir|
      pdf = File.join(dir, 'out.pdf')
      docx = File.join(dir, 'out.docx')

      assert_equal pdf, Imprimatur.pdf(pdf, page_size: :letter) { it.paragraph 'Letter.' }
      assert_equal docx, Imprimatur.docx(docx) { it.paragraph 'Word.' }
      assert_includes File.binread(pdf), '/MediaBox [0 0 612 792]'
      assert_includes ZipReader.entries(File.binread(docx))['word/document.xml'], 'Word.'
    end
    assert Imprimatur.pdf { it.heading 'Bytes' }.start_with?('%PDF')
  end

  def test_right_to_left_text_goes_to_docx_but_not_the_standard_pdf_fonts
    doc = Imprimatur::Document.new
    doc.paragraph 'مرحبا بالعالم', direction: :rtl

    assert_includes ZipReader.entries(doc.to_docx)['word/document.xml'], '<w:bidi/>'
    error = assert_raises(Imprimatur::Error) { doc.to_pdf }
    assert_match(/U\+0645.*Helvetica/, error.message)
  end

  def test_rejects_invalid_content
    doc = Imprimatur::Document.new

    assert_raises(Imprimatur::Error) { doc.heading 'x', level: 4 }
    assert_raises(Imprimatur::Error) { doc.paragraph "bell\a" }
    assert_raises(Imprimatur::Error) { doc.table [] }
    assert_raises(Imprimatur::Error) { doc.paragraph 'x', color: 'green' }
    assert_raises(ArgumentError) { doc.paragraph 'x', align: :justify }
    assert_raises(ArgumentError) { Imprimatur::Document.new(page_size: :a3) }
    assert_raises(ArgumentError) { doc.paragraph 'x', font: 'Comic Sans' }
  end
end
