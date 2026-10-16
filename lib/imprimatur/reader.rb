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
    # File signatures a document's bytes start with; a String that starts
    # with neither is taken for a path.
    SIGNATURES = ["%PDF-".b, "PK\x03\x04".b, "PK\x05\x06".b].freeze
    private_constant :SIGNATURES

    # @see Imprimatur.markdown
    def self.markdown(document, **) = Native.markdown(read(document), *limits(**))

    # @see Imprimatur.content
    def self.content(document, **)
      JSON.parse(Native.content(read(document), *limits(**)), symbolize_names: true)
    end

    # @see Imprimatur.images
    def self.images(document, **)
      Native.images(read(document), *limits(**)).map do |data, description|
        found = JSON.parse(description, symbolize_names: true)
        Image.new(data:, **Image.members.drop(1).to_h { [it, found[it]] })
      end
    end

    # @api private
    def self.limits(timeout: 60, max_bytes: nil, max_inflated: nil)
      [max_bytes, max_inflated, timeout && Float(timeout)]
    end

    # @api private
    def self.read(document)
      case document
      when Pathname then File.binread(document)
      when String then bytes?(document) ? document : File.binread(document)
      else
        raise TypeError, "expected a path, bytes or an IO, not #{document.class}" unless document.respond_to?(:read)

        document.read.to_s.b
      end
    end

    # @api private
    def self.bytes?(string)
      head = string.byteslice(0, 1024).b
      SIGNATURES.any? { head.start_with?(it) } || head.include?("%PDF-") || head.include?("\0")
    end
  end
end
