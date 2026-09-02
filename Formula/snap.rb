# Homebrew formula for snap. Intended for a personal tap (e.g.
# `cesarferreira/homebrew-tap`) rather than homebrew-core — see the Install
# section of README.md for the `brew install` command this backs.
#
# Installs the prebuilt binary from this repo's GitHub Release (published by
# .github/workflows/release.yml) rather than building from source, so users
# don't need a Rust toolchain. The installed binary is named `snap`, not
# `snap-macos` (the crates.io name, which was taken).
#
# release.yml pushes the version/checksum update to homebrew-tap automatically
# on every tag; this copy is kept in sync manually as a reference.
class Snap < Formula
  desc "Fast, minimal macOS window manipulation from the terminal"
  homepage "https://github.com/cesarferreira/snap"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/cesarferreira/snap/releases/download/v#{version}/snap-aarch64-apple-darwin.tar.gz"
      sha256 "c528945ee29cde6fe7509824131c9397b0cd2b26e19956b46df9ea3216d8dd93"
    end
    on_intel do
      url "https://github.com/cesarferreira/snap/releases/download/v#{version}/snap-x86_64-apple-darwin.tar.gz"
      sha256 "ea0ec86ccdeb5e6288e1a7ab973090c5242c26add886c8541725e525e42ed1f1"
    end
  end

  def install
    bin.install "snap"
  end

  test do
    assert_match "snap", shell_output("#{bin}/snap --version")
  end
end
