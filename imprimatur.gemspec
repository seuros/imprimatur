# frozen_string_literal: true

require_relative 'lib/imprimatur/version'

Gem::Specification.new do |spec|
  # Set by `rake gems:native`: the gem then carries the prebuilt extension for
  # that platform. Unset, it is the source gem, which builds it with Cargo.
  platform = ENV.fetch('IMPRIMATUR_PLATFORM', nil)

  spec.name = 'imprimatur'
  spec.version = Imprimatur::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['terminale@gmail.com']

  spec.summary = 'XLSX, PDF and DOCX authoring with a Ruby DSL. Let it be printed.'
  spec.description = 'A Ruby DSL over a native core that writes Excel workbooks, and PDF and Word documents ' \
                     'from one model, with its own XML, zip and PDF writers: no third-party format stack. ' \
                     'Precompiled for Linux, macOS and FreeBSD; CRuby only.'
  spec.homepage = 'https://github.com/seuros/imprimatur'
  spec.license = 'BSD-3-Clause'

  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['bug_tracker_uri'] = "#{spec.homepage}/issues"
  spec.metadata['changelog_uri'] = "#{spec.homepage}/blob/master/CHANGELOG.md"
  spec.metadata['rubygems_mfa_required'] = 'true'

  spec.files = Dir['lib/**/*.rb', 'README.md', 'CHANGELOG.md', 'LICENSE.txt']
  spec.require_paths = ['lib']

  if platform
    spec.platform = Gem::Platform.new(platform)
    spec.files += Dir['lib/imprimatur/[0-9]*/imprimatur_native.{so,bundle}']
    # The binary is built for one Ruby ABI; other Rubies build the source gem.
    spec.required_ruby_version = ['>= 4.0', '< 4.1.dev']
  else
    # The core's unit tests (`mod tests;` under #[cfg(test)]) are not needed to build.
    spec.files += Dir['ext/**/*.{rb,rs,toml,txt}', 'ext/imprimatur_native/core/{README.md,LICENSE}',
                      'Cargo.toml', 'Cargo.lock'].reject { it.match?(%r{/tests(/|\.rs\z)|/examples/}) }
    spec.extensions = ['ext/imprimatur_native/extconf.rb']
    spec.required_ruby_version = '>= 4.0'
    spec.add_dependency 'rb_sys', '~> 0.9.130'
  end
end
