# frozen_string_literal: true

module Imprimatur
  # Keyword handling only; parsing, limits and rendering live in the native core.
  module PdfReader
    def self.info(document, max_bytes: nil, **limits)
      JSON.parse(Native.pdf_info(Reader.read(document, max_bytes), Reader.limits(max_bytes:, **limits)),
                 symbolize_names: true)
    end

    def self.pages(document, first_page: 1, last_page: 0xFFFFFFFF, max_pages: 500,
                   max_page_bytes: 256 * 1024, max_output_bytes: 8 * 1024 * 1024,
                   max_bytes: nil, **limits)
      options = [first_page, last_page, max_pages, max_page_bytes, max_output_bytes]
      JSON.parse(Native.pdf_pages(Reader.read(document, max_bytes), Reader.limits(max_bytes:, **limits), options),
                 symbolize_names: true)
    end

    def self.render(document, page:, max_side: 1568, max_pixels: 4_000_000,
                    max_output_bytes: 8 * 1024 * 1024, grayscale: true,
                    memory_bytes: 1024 * 1024 * 1024, max_bytes: nil, **limits)
      Native.render_pdf_page(Reader.read(document, max_bytes), Reader.limits(max_bytes:, **limits),
                            [page, max_side, max_pixels, max_output_bytes, grayscale],
                            RENDER_WORKER, memory_bytes)
    end
  end
end
