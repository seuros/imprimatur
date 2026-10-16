# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'minitest/test_task'

GEMSPEC = Gem::Specification.load('imprimatur.gemspec')
CARGO_MANIFEST = 'Cargo.toml'
# RbSys::ExtensionTask would point cargo at ext_dir (where extconf.rb lives);
# the crate itself is in ffi/.
ENV['RB_SYS_CARGO_MANIFEST_DIR'] ||= File.expand_path('ext/imprimatur_native/ffi', __dir__)

Minitest::TestTask.create(:test) do |t|
  t.test_globs = ['test/**/*_test.rb']
end
# Native only: the suite needs the extension.
task test: :compile

require 'rb_sys/extensiontask'

RbSys::ExtensionTask.new('imprimatur_native', GEMSPEC) do |ext|
  ext.ext_dir = 'ext/imprimatur_native'
  ext.lib_dir = 'lib/imprimatur'
end

namespace :test do
  desc 'Unit tests of the core library'
  task :core do
    sh 'cargo', 'test', '--manifest-path', CARGO_MANIFEST, '--package', 'imprimatur'
  end

  desc 'The core library, then the Ruby suite on the compiled extension'
  task all: %i[core test]
end

desc 'dictator, cargo fmt --check and clippy'
task :lint do
  sh 'dictator', 'lint', '.'
  sh 'cargo', 'fmt', '--manifest-path', CARGO_MANIFEST, '--all', '--check'
  sh 'cargo', 'clippy', '--manifest-path', CARGO_MANIFEST, '--workspace', '--all-targets', '--', '-D', 'warnings'
end

GITHUB_REPO = 'seuros/imprimatur'
RUBYGEMS_VERSIONS_URL = "https://rubygems.org/api/v1/versions/#{GEMSPEC.name}.json".freeze
# What the release workflow builds: the source gem and one gem per platform.
# FreeBSD's platform carries the release it was built on.
RELEASE_PLATFORMS = ['ruby', 'x86_64-linux-gnu', 'aarch64-linux-gnu', 'x86_64-linux-musl', 'aarch64-linux-musl',
                     'arm64-darwin', 'x86_64-darwin', /freebsd/].freeze

# The gems in pkg/ for a version, by the platform each was built for.
def built_gems(version)
  Dir["pkg/#{GEMSPEC.name}-#{version}.gem", "pkg/#{GEMSPEC.name}-#{version}-*.gem"]
    .to_h { [Gem::Package.new(it).spec.platform.to_s, it] }
end

# The release platforms none of `gems` was built for.
def missing_platforms(gems)
  RELEASE_PLATFORMS.select { gems.keys.grep(it).empty? }
end

# The platforms rubygems.org already has for a version.
def published_platforms(version)
  require 'json'
  require 'net/http'

  response = Net::HTTP.get_response(URI(RUBYGEMS_VERSIONS_URL))
  return [] if response.is_a?(Net::HTTPNotFound)

  abort "rubygems.org returned #{response.code} for #{RUBYGEMS_VERSIONS_URL}" unless response.is_a?(Net::HTTPSuccess)

  JSON.parse(response.body).select { it['number'] == version }.map { it['platform'] }
end

namespace :gems do
  # A task argument, not IMPRIMATUR_PLATFORM: set for the whole rake process,
  # that would turn the gemspec into a platform gem for compile's bundler/setup too.
  desc 'Build the precompiled gem for this machine (gems:native[platform] overrides the platform)'
  task :native, [:platform] => :compile do |_task, args|
    ruby_abi = RUBY_VERSION[/\A\d+\.\d+/]
    binary = "imprimatur_native.#{RbConfig::CONFIG['DLEXT']}"
    staged = "lib/imprimatur/#{ruby_abi}/#{binary}"
    local = Gem::Platform.local
    # A darwin version would limit the gem to that one macOS release.
    platform = args[:platform] || (local.os == 'darwin' ? "#{local.cpu}-darwin" : local.to_s)

    mkdir_p File.dirname(staged)
    cp "lib/imprimatur/#{binary}", staged
    mkdir_p 'pkg'
    Bundler.with_unbundled_env do
      sh({ 'IMPRIMATUR_PLATFORM' => platform }, 'gem', 'build', 'imprimatur.gemspec',
         '--output', "pkg/imprimatur-#{GEMSPEC.version}-#{platform}.gem")
    end
  ensure
    rm_rf File.dirname(staged) if staged
  end

  desc 'Download the gems attached to the GitHub release for VERSION (default: current) into pkg/'
  task :pull_all, [:version] do |_task, args|
    version = args[:version] || GEMSPEC.version.to_s
    tag = "v#{version}"
    sh 'gh', 'release', 'download', tag, '--repo', GITHUB_REPO, '--pattern', '*.gem', '--dir', 'pkg', '--clobber'

    gems = built_gems(version)
    missing = missing_platforms(gems)
    abort "Release #{tag} has no gem for #{missing.map(&:inspect).join(', ')}" unless missing.empty?

    gems.each_value { puts "#{it} (#{File.size(it)} bytes)" }
  end

  desc 'Push the gems in pkg/ for VERSION (default: current) to rubygems.org, skipping platforms already there'
  task :push_all, [:version] do |_task, args|
    version = args[:version] || GEMSPEC.version.to_s
    gems = built_gems(version)
    missing = missing_platforms(gems)
    unless missing.empty?
      abort "pkg/ has no gem for #{missing.map(&:inspect).join(', ')}; run rake gems:pull_all[#{version}] first"
    end

    published = published_platforms(version)
    gems.each do |platform, path|
      if published.include?(platform)
        puts "skip #{path}: already on rubygems.org"
      else
        sh 'gem', 'push', path
      end
    end
  end
end

task default: 'test:all'

# Bundler's release would push only the source gem.
Rake::Task['release'].clear
desc 'Releases come from release-please: merge its PR, then rake gems:pull_all gems:push_all'
task :release do
  abort 'Merge the release-please PR; once the release has its gems, run `rake gems:pull_all gems:push_all`'
end
