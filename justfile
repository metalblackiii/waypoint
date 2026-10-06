# List available recipes
[default]
_default:
    @just --list

# Build the waypoint binary
build:
    cargo build

# Build release binary
release:
    cargo build --release

# Install waypoint into Cargo's bin directory
install:
    cargo install --locked --path .

# Uninstall waypoint from Cargo's bin directory
uninstall:
    cargo uninstall waypoint

# Run the linter
lint:
    cargo clippy --all-targets --all-features

# Check formatting
fmt-check:
    cargo fmt --check

# Format code
fmt:
    cargo fmt

# Run all tests
test:
    cargo test

# Run the pi adapter tests (Node >= 22.18 for .ts type stripping)
test-pi:
    node --test plugins/waypoint/pi/*-test.mjs

# Scan current project
scan:
    cargo run -- scan

# Check waypoint status
status:
    cargo run -- status
