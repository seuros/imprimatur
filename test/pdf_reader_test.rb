# frozen_string_literal: true

require_relative 'test_helper'

class PdfReaderTest < Minitest::Test
  def test_bounded_native_reads_and_rendering_through_the_public_keywords
    pdf = Imprimatur.pdf do |doc|
      doc.paragraph('First')
      doc.page_break
      doc.paragraph('Second')
    end

    assert_equal({ page_count: 2, encrypted: false }, Imprimatur.pdf_info(pdf))
    result = Imprimatur.pdf_pages(pdf, first_page: 2, last_page: 2, max_page_bytes: 3)
    assert_equal 2, result[:page_count]
    assert_equal [{ page: 2, status: 'text', text: 'Sec', images: 0, truncated: true,
                    reason: 'the page text limit was reached' }], result[:pages]
    assert_equal 2, Imprimatur.pdf_info(pdf, max_inflated: 0)[:page_count]
    assert_raises(ArgumentError) { Imprimatur.pdf_pages(pdf, first_page: 0) }
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.pdf_pages(pdf, timeout: 0) }
    png = Imprimatur.render_pdf_page(pdf, page: 1, max_side: 300)
    assert_equal Encoding::BINARY, png.encoding
    assert png.start_with?("\x89PNG\r\n\x1a\n".b)
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.render_pdf_page(pdf, page: 1, max_pixels: 1) }
    assert_raises(ArgumentError) { Imprimatur.render_pdf_page(pdf, page: 1, memory_bytes: 0) }
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.render_pdf_page(pdf, page: 1, memory_bytes: 1) }
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.render_pdf_page(pdf, page: 1, timeout: 0.001) }
    # The host stays usable after worker termination.
    assert_equal 2, Imprimatur.pdf_info(pdf)[:page_count]
  end

  def test_a_thrown_stream_limit_keeps_healthy_neighbors
    pdf = Imprimatur.pdf do |doc|
      doc.paragraph('Healthy first page')
      doc.page_break
      doc.paragraph('x' * 2000)
      doc.page_break
      doc.paragraph('Healthy last page')
    end

    result = Imprimatur.pdf_pages(pdf, max_inflated: 1024)
    assert_equal 3, result[:page_count]
    assert_equal %w[text failed text], result[:pages].map { it[:status] }
    assert_equal ['Healthy first page', 'Healthy last page'],
                 result[:pages].values_at(0, 2).map { it[:text] }
    assert_includes result[:pages][1][:reason], 'limit'
    assert_raises(ArgumentError) { Imprimatur.pdf_info(pdf, memory_bytes: 0) }
    assert_raises(ArgumentError) { Imprimatur.pdf_pages(pdf, memory_bytes: 0) }
  end
end
