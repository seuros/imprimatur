# frozen_string_literal: true

require 'json'

module Imprimatur
  # An image found in a document, as a file: {#data} is what a file of
  # {#format} holds. On a PDF page, {#page} and {#bounds} (points from the
  # page's top-left corner) say where; in a Word or Excel package, {#part}
  # names the medium, and {#sheet} the sheet that shows it.
  Image = Data.define(:data, :format, :width, :height, :page, :bounds, :part, :sheet)

  # Documents in: Markdown, their structure, their images. See
  # {Imprimatur.markdown}, {Imprimatur.content} and {Imprimatur.images}.
  module Reader
    # File signatures a document's bytes start with; a String that is not
    # binary and starts with neither is taken for a path.
    SIGNATURES = ["%PDF-".b, "PK\x03\x04".b, "PK\x05\x06".b, "\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".b].freeze
    private_constant :SIGNATURES

    # The largest document read in unless told, as the native core's own limit.
    MAX_BYTES = 512 * 1024 * 1024
    private_constant :MAX_BYTES

    # @see Imprimatur.markdown
    def self.markdown(document, max_bytes: nil, **)
      Native.markdown(read(document, max_bytes), *limits(max_bytes:, **))
    end

    # @see Imprimatur.content
    def self.content(document, styles: false, max_bytes: nil, **)
      bytes = read(document, max_bytes)
      JSON.parse(Native.content(bytes, *limits(max_bytes:, **), styles ? true : false), symbolize_names: true)
    end

    # @see Imprimatur.images
    def self.images(document, max_bytes: nil, **)
      Native.images(read(document, max_bytes), *limits(max_bytes:, **)).map do |data, description|
        found = JSON.parse(description, symbolize_names: true)
        Image.new(data:, **Image.members.drop(1).to_h { [it, found[it]] })
      end
    end

    # @api private
    def self.limits(timeout: 60, max_bytes: nil, max_inflated: nil, drop_repeated: false)
      [max_bytes, max_inflated, timeout && Float(timeout), drop_repeated ? true : false]
    end

    # A path or an IO is read no further than +max_bytes+ (one byte past it
    # tells it is passed): an endless stream or a huge file stops there.
    #
    # @api private
    def self.read(document, max_bytes = nil)
      limit = max_bytes.is_a?(Integer) && max_bytes >= 0 ? max_bytes : MAX_BYTES
      case document
      when Pathname then file(document, limit)
      when String then bytes?(document) ? document : file(document, limit)
      else
        raise TypeError, "expected a path, bytes or an IO, not #{document.class}" unless document.respond_to?(:read)

        within(document.read(limit + 1).to_s.b, limit)
      end
    end

    # A binary String (what +File.binread+, a binary IO or a download give)
    # is bytes, whatever it holds: uploaded bytes never name a file to read.
    #
    # @api private
    def self.bytes?(string)
      return true if string.encoding == Encoding::BINARY

      head = string.byteslice(0, 1024).b
      SIGNATURES.any? { head.start_with?(it) } || head.include?("%PDF-") || head.include?("\0")
    end

    # A regular file's bytes. Opened without blocking and checked through
    # the open file, so a pipe cannot hang the read, a device such as
    # +/dev/zero+ is refused, and the file cannot be swapped in between.
    #
    # @api private
    def self.file(path, limit)
      File.open(path, File::RDONLY | File::NONBLOCK | File::BINARY) do |io|
        stat = io.stat
        raise UnsupportedDocument, "cannot read this document: #{path} is not a regular file" unless stat.file?

        within(stat.size > limit ? '' : io.read(limit + 1).to_s, limit, stat.size)
      end
    end

    # @api private
    def self.within(bytes, limit, size = bytes.bytesize)
      raise LimitExceeded, 'the document exceeds the input size limit' if [size, bytes.bytesize].max > limit

      bytes
    end
  end
end
