# frozen_string_literal: true

module Imprimatur
  # A document Excel (or Word, or a PDF reader) would reject: an invalid or
  # duplicate sheet name, a cell out of range, text over a limit, NaN. Also
  # a document {Imprimatur.markdown} cannot read. Raised by the native core.
  class Error < StandardError; end

  # A PDF without a text layer: its pages are images (scans), which need OCR.
  class NoTextLayer < Error; end

  # A PDF encrypted with a user password; only an empty one is tried.
  class PasswordRequired < Error; end

  # A format {Imprimatur.markdown} does not read: legacy +.xls+ and +.doc+,
  # binary +.xlsb+, presentations, anything not a PDF, XLSX or DOCX.
  class UnsupportedDocument < Error; end

  # The read hit one of its limits (size, decompressed size, nesting, time),
  # or was cancelled.
  class LimitExceeded < Error; end
end
