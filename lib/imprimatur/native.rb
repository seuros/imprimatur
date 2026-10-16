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
