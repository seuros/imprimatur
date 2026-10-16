# frozen_string_literal: true

require_relative 'test_helper'
require 'timeout'

class ReaderTest < Minitest::Test
  def workbook
    Imprimatur.xlsx do |book|
      book.sheet('Sales') do |sheet|
        sheet.row 'Region', 'Amount'
        sheet.row 'North | West', 12.5
        sheet.comment 'A1', 'Where it sold', author: 'Finance'
      end
    end
  end

  # A page drawing a 2 by 2 gray image at (72, 600), 100 by 50 points.
  def figure
    objects = [
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R ' \
      '/Resources << /XObject << /A 5 0 R >> >> >>',
      stream('', 'q 100 0 0 50 72 600 cm /A Do Q'),
      stream('/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray ' \
             '/BitsPerComponent 8', "\x00\x55\xAA\xFF".b)
    ]
    pdf = "%PDF-1.7\n".b
    offsets = objects.each_with_index.map do |object, index|
      pdf.size.tap { pdf << "#{index + 1} 0 obj\n".b << object.b << "\nendobj\n".b }
    end
    xref = pdf.size
    pdf << "xref\n0 #{objects.size + 1}\n0000000000 65535 f \n"
    offsets.each { pdf << format("%010d 00000 n \n", it) }
    pdf << "trailer\n<< /Size #{objects.size + 1} /Root 1 0 R >>\nstartxref\n#{xref}\n%%EOF\n"
  end

  def stream(dict, data) = "<< #{dict} /Length #{data.bytesize} >>\nstream\n".b + data.b + "\nendstream".b

  def test_reads_workbooks_documents_and_pdfs
    assert_equal <<~MARKDOWN, Imprimatur.markdown(workbook)
      ## Sales

      | Region | Amount |
      | --- | --- |
      | North \\| West | 12.5 |

      Comments:

      - A1 (Finance): Where it sold
    MARKDOWN

    docx = Imprimatur.docx { it.heading('Report') && it.paragraph('Body text.') }
    pdf = Imprimatur.pdf { it.heading('Report') && it.paragraph('Body text.') }

    assert_equal "# Report\n\nBody text.\n", Imprimatur.markdown(docx)
    assert_equal "# Report\n\nBody text.\n", Imprimatur.markdown(pdf)
    assert_equal Encoding::UTF_8, Imprimatur.markdown(pdf).encoding
  end

  def test_reads_the_structure
    sheet = Imprimatur.content(workbook)[:sheets].first

    assert_equal 'Sales', sheet[:name]
    assert_equal({ cell: 'B2', type: 'number', value: 12.5, text: '12.5' }, sheet[:rows][1][:cells][1])
    assert_equal [{ cell: 'A1', author: 'Finance', text: 'Where it sold' }], sheet[:comments]

    docx = Imprimatur.content(Imprimatur.docx { it.heading('Report') && it.paragraph('Body text.') })

    assert_equal 'docx', docx[:format]
    assert_equal [{ type: 'heading', level: 1, text: 'Report' }, { type: 'paragraph', text: 'Body text.' }],
                 docx[:blocks]

    page = Imprimatur.content(Imprimatur.pdf { it.paragraph('Body text.') })[:pages].first

    assert_equal 'Body text.', page[:blocks].first[:text]
    assert_operator page[:blocks].first[:bounds][:left], :<, page[:blocks].first[:bounds][:right]
  end

  def test_extracts_images
    images = Imprimatur.images(figure)

    assert_equal 1, images.size
    image = images.first

    assert_equal ['png', 2, 2, 1, nil], [image.format, image.width, image.height, image.page, image.sheet]
    assert_equal({ left: 72.0, top: 142.0, right: 172.0, bottom: 192.0 }, image.bounds)
    assert_equal Encoding::BINARY, image.data.encoding
    assert image.data.start_with?("\x89PNG".b)
    assert_empty Imprimatur.images(workbook)

    yielded = []
    Imprimatur.images(figure) { yielded << it }

    assert_equal images, yielded
    assert_raises(Imprimatur::UnsupportedDocument) { Imprimatur.images("\0\0 not a document".b) }
  end

  def test_reads_right_to_left_scripts_back
    book = Imprimatur.xlsx do |workbook|
      workbook.sheet('מכירות') do |sheet|
        sheet.right_to_left!
        sheet.row 'אזור', 'منطقه', 'ܐܬܪܐ', style: Imprimatur.style(direction: :rtl)
      end
    end

    expected = "## מכירות\n\n| אזור | منطقه | ܐܬܪܐ |\n| --- | --- | --- |\n"

    assert_equal expected, Imprimatur.markdown(book)

    docx = Imprimatur.docx { it.paragraph('שלום עולם', direction: :rtl) }

    assert_equal [{ type: 'paragraph', text: 'שלום עולם' }], Imprimatur.content(docx)[:blocks]
  end

  def test_reads_paths_and_io
    Dir.mktmpdir do |dir|
      path = File.join(dir, 'sales.xlsx')
      File.binwrite(path, workbook)

      assert_includes Imprimatur.markdown(path), '## Sales'
      assert_includes Imprimatur.markdown(Pathname(path)), '## Sales'
      File.open(path, 'rb') { assert_includes Imprimatur.markdown(it), '## Sales' }
    end
  end

  def test_raises_what_callers_handle
    scanned = Imprimatur.pdf { it.paragraph(' ') }

    assert_raises(Imprimatur::NoTextLayer) { Imprimatur.markdown(scanned) }
    assert_raises(Imprimatur::UnsupportedDocument) { Imprimatur.markdown("\0\0 not a document".b) }
    damaged = assert_raises(Imprimatur::Error) { Imprimatur.markdown("PK\x03\x04 truncated".b) }

    assert_match(/malformed zip/, damaged.message)
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.markdown(workbook, max_bytes: 10) }
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.markdown(workbook, max_inflated: 100) }
    assert_raises(Errno::ENOENT) { Imprimatur.markdown('/no/such/file.pdf') }
    assert_raises(TypeError) { Imprimatur.markdown(42) }
    assert_raises(ArgumentError) { Imprimatur.markdown(workbook, timeout: -1) }
    assert_kind_of Imprimatur::Error, Imprimatur::NoTextLayer.new
  end

  def test_a_timeout_cancels_the_read
    huge = Imprimatur.xlsx do |book|
      sheet = book.sheet('Huge')
      200_000.times { |row| sheet.row "row #{row}", row, 'text ' * 8 }
    end
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    assert_raises(Timeout::Error) { Timeout.timeout(0.02) { Imprimatur.markdown(huge) } }
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started

    assert_operator elapsed, :<, 0.5, 'cancelled, not run to the end'
    assert_raises(Imprimatur::LimitExceeded) { Imprimatur.markdown(huge, timeout: 0.01) }
  end

  def test_releases_the_gvl
    # A large workbook keeps the reader busy while another thread counts.
    big = Imprimatur.xlsx do |book|
      sheet = book.sheet('Big')
      20_000.times { |row| sheet.row "row #{row}", row, row * 1.5, 'text ' * 5 }
    end
    ticks = 0
    started = Queue.new
    counter = Thread.new do
      started << true
      loop do
        ticks += 1
        sleep 0.001
      end
    end
    started.pop
    before = ticks
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    Imprimatur.markdown(big)
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - elapsed
    during = ticks - before
    counter.kill

    # Held, the GVL would let the counter tick once at most during the read.
    assert_operator during, :>, 2, "the other thread ticked #{during} times during the #{elapsed}s read"
  end
end
