.PHONY: all build build-release install install-release clean test check fmt lint run demo release formula-sha256

LEVEL ?= minor

# Default target
all: check build test

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# Install debug binary to ~/.cargo/bin
install:
	CARGO_INCREMENTAL=0 cargo install --path . --locked --bins --debug --force

# Install release binary to ~/.cargo/bin
install-release:
	CARGO_INCREMENTAL=0 cargo install --path . --locked --bins --force

# Clean build artifacts
clean:
	cargo clean

# Run tests
test:
	cargo test

# Run clippy and check
check:
	cargo check
	cargo clippy -- -D warnings

# Format code
fmt:
	cargo fmt

# Lint (check formatting)
lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings

# Run with arguments (usage: make run ARGS="--hello")
run:
	cargo run -- $(ARGS)

# Quick demo
demo: install
	@echo "=== snap demo ==="
	snap --help

# Bump version, regenerate CHANGELOG.md, tag, publish, and push (requires cargo-release + git-cliff)
release:
	cargo release $(LEVEL) --execute --no-confirm

# Print sha256 checksums for the current version's release tarballs, to
# paste into Formula/snap.rb (Homebrew release checklist). Run this after
# `make release` once CI has published the new tag's artifacts.
formula-sha256:
	@version=$$(grep '^version' Cargo.toml | head -1 | cut -d '"' -f2); \
	for target in aarch64-apple-darwin x86_64-apple-darwin; do \
		url="https://github.com/cesarferreira/snap/releases/download/v$$version/snap-$$target.tar.gz"; \
		echo "$$target: $$(curl -sL $$url | shasum -a 256 | cut -d' ' -f1)"; \
	done
