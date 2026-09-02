# Homebrew formula for snap. Intended for a personal tap (e.g.
# `cesarferreira/homebrew-tap`) rather than homebrew-core — see the Install
# section of README.md for the `brew install` command this backs.
#
# Installs the prebuilt binary from this repo's GitHub Release (published by
# .github/workflows/release.yml) rather than building from source, so users
# don't need a Rust toolchain. The installed binary is named `snap`, not
# `snap-macos` (the crates.io name, which was taken).
#
# Release checklist: after `make release` finishes and CI has published the
# new tag's artifacts, run `make formula-sha256` and paste the two checksums
# below, then bump `version`.
class Snap < Formula
  desc "Fast, minimal macOS window manipulation from the terminal"
  homepage "https://github.com/cesarferreira/snap"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/cesarferreira/snap/releases/download/v#{version}/snap-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_AARCH64_SHA256" # updated by `make formula-sha256` each release
    end
    on_intel do
      url "https://github.com/cesarferreira/snap/releases/download/v#{version}/snap-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_X86_64_SHA256" # updated by `make formula-sha256` each release
    end
  end

  def install
    bin.install "snap"
  end

  test do
    assert_match "snap", shell_output("#{bin}/snap --version")
  end
end
