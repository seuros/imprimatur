# frozen_string_literal: true

# imprimatur is native only: there is no pure Ruby engine to fall back to.
#
# Platform gems ship the binary under the Ruby ABI version; a dev build (rake
# compile) or a source install leaves it next to this file.
shipped = File.join(__dir__, RUBY_VERSION[/\A\d+\.\d+/], "imprimatur_native.#{RbConfig::CONFIG['DLEXT']}")
begin
  if File.exist?(shipped)
    require shipped
  else
    require 'imprimatur/imprimatur_native'
  end
rescue LoadError => e
  raise LoadError, "imprimatur: the native extension did not load (#{e.message}). " \
                   'Install a platform gem, or build it with `rake compile` (toolchain: https://rustup.rs).'
end

module Imprimatur
  # Always use the companion of the loaded extension, never PATH.
  RENDER_WORKER = File.join(File.dirname($LOADED_FEATURES.find do |path|
    path.end_with?("/imprimatur_native.#{RbConfig::CONFIG['DLEXT']}")
  end), 'imprimatur-render-worker').freeze
end
