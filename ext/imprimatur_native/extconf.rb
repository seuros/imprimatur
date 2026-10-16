# frozen_string_literal: true

# imprimatur is native only: without the extension there is no gem, so every
# missing prerequisite stops the install with what to do about it.
def fail_build(reason)
  abort "imprimatur: #{reason}"
end

def command?(command) = system(command, out: File::NULL, err: File::NULL)

# rb_sys writes a GNU Makefile; RubyGems runs the system make (BSD make here).
BSD = RUBY_PLATFORM.include?('bsd')

fail_build("#{RUBY_ENGINE} is not supported; imprimatur needs CRuby") unless RUBY_ENGINE == 'ruby'
fail_build('Cargo not found; install Rust (https://rustup.rs) or use a platform gem') unless command?('cargo --version')
fail_build('GNU make not found (pkg install gmake)') if BSD && !command?('gmake --version')

require 'mkmf'
require 'rb_sys/mkmf'

create_rust_makefile('imprimatur/imprimatur_native') do |r|
  r.ext_dir = 'ffi'
  r.profile = ENV.fetch('RB_SYS_CARGO_PROFILE', :release).to_sym
end

# BSD make reads BSDmakefile first (GNU make ignores it): delegate to gmake.
if BSD
  File.write('BSDmakefile', <<~MAKE)
    GMAKE_VARS =
    .for var in DESTDIR sitearchdir sitelibdir
    .if defined(${var})
    GMAKE_VARS += ${var}="${${var}}"
    .endif
    .endfor

    # BSD make exports its own job flags (-J) in MAKEFLAGS; gmake rejects them.
    GMAKE = env -u MAKEFLAGS -u MFLAGS gmake -f Makefile

    all:
    \t${GMAKE} ${GMAKE_VARS}

    .DEFAULT:
    \t${GMAKE} ${GMAKE_VARS} $@
  MAKE
end
