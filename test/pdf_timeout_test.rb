# frozen_string_literal: true

require_relative 'test_helper'

class PdfTimeoutTest < Minitest::Test
  def test_real_timeout_recovers_the_healthy_later_page
    pdf = slow_middle_pdf
    # First prove that this document actually times out after the first page,
    # rather than accidentally testing a normally completed batch.
    original = Imprimatur.pdf_pages(pdf, timeout: 0.5)
    assert_equal %w[text skipped skipped], original[:pages].map { it[:status] }
    assert_equal 'Healthy first page', original[:pages][0][:text]

    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    recovered = Imprimatur.pdf_pages(pdf, timeout: 5, batch_timeout: 0.5)
    assert_operator Process.clock_gettime(Process::CLOCK_MONOTONIC) - started, :<, 5
    assert_equal [1, 2, 3], recovered[:pages].map { it[:page] }
    assert_equal 'text', recovered[:pages][0][:status]
    assert_equal 'text', recovered[:pages][2][:status]
    # The graphics-only middle page may finish in its singleton attempt.
    # Recovery need not fail it just because the original batch timed out.
    assert_includes %w[empty skipped], recovered[:pages][1][:status]
    assert_equal 'Healthy first page', recovered[:pages][0][:text]
    assert_equal 'Healthy third page', recovered[:pages][2][:text]
    # Caller remains usable after both worker kills.
    assert_equal 3, Imprimatur.pdf_info(pdf)[:page_count]
  end

  def test_batch_timeout_is_optional_positive_and_finite
    pdf = Imprimatur.pdf { |doc| doc.paragraph('Healthy') }
    assert_equal Imprimatur.pdf_pages(pdf), Imprimatur.pdf_pages(pdf, batch_timeout: nil)
    assert_equal 'Healthy', Imprimatur.pdf_pages(pdf, timeout: 2, batch_timeout: 1)[:pages][0][:text]
    [0, -1, Float::INFINITY, Float::NAN].each do |seconds|
      assert_raises(ArgumentError) { Imprimatur.pdf_pages(pdf, batch_timeout: seconds) }
    end
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.pdf_pages(pdf, timeout: 0, batch_timeout: 1) }
  end

  private

  def slow_middle_pdf
    objects = [
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>'
    ]
    3.times do |index|
      objects << "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " \
                 "/Resources << /Font << /F1 6 0 R >> >> /Contents #{7 + index} 0 R >>"
    end
    objects << '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'
    [
      'BT /F1 12 Tf 50 700 Td (Healthy first page) Tj ET',
      "q Q\n" * 20_000_000,
      'BT /F1 12 Tf 50 700 Td (Healthy third page) Tj ET'
    ].each do |stream|
      compressed = Zlib::Deflate.deflate(stream)
      objects << "<< /Length #{compressed.bytesize} /Filter /FlateDecode >>\nstream\n".b +
                 compressed + "\nendstream"
    end
    pdf = "%PDF-1.4\n".b
    offsets = objects.map.with_index do |object, index|
      offset = pdf.bytesize
      pdf << "#{index + 1} 0 obj\n" << object << "\nendobj\n"
      offset
    end
    start = pdf.bytesize
    pdf << "xref\n0 #{objects.size + 1}\n0000000000 65535 f \n"
    offsets.each { pdf << format("%010d 00000 n \n", it) }
    pdf << "trailer\n<< /Size #{objects.size + 1} /Root 1 0 R >>\nstartxref\n#{start}\n%%EOF\n"
  end
end
