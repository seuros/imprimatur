# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'imprimatur'
require 'minitest/autorun'
require 'tmpdir'
require 'zlib'

# Reads the packages imprimatur writes, with stdlib Zlib: local headers carry
# the sizes (no data descriptors), entries are stored or raw deflate.
module ZipReader
  LOCAL_HEADER = 0x04034b50

  # @return [Hash{String => String}] part name => contents
  def self.entries(bytes)
    entries = {}
    offset = 0
    while bytes.byteslice(offset, 4).unpack1('V') == LOCAL_HEADER
      method, crc, compressed, size, name_length, extra_length =
        bytes.byteslice(offset + 8, 22).unpack('v x4 V V V v v')
      name = bytes.byteslice(offset + 30, name_length)
      offset += 30 + name_length + extra_length
      data = inflate(bytes.byteslice(offset, compressed), method)
      raise "#{name}: size mismatch" unless data.bytesize == size
      raise "#{name}: CRC mismatch" unless Zlib.crc32(data) == crc

      entries[name] = data.force_encoding(Encoding::UTF_8)
      offset += compressed
    end
    entries
  end

  def self.inflate(data, method)
    case method
    when 0 then data
    when 8 then Zlib::Inflate.new(-Zlib::MAX_WBITS).inflate(data)
    else raise "unknown compression method #{method}"
    end
  end
end
