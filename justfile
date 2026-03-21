# List available recipes
[default]
_default:
    @just --list

# Build the waypoint binary
build:
    cargo build

# Install waypoint into Cargo's bin directory
install:
    cargo install --locked --path .

# Uninstall waypoint from Cargo's bin directory
uninstall:
    cargo uninstall waypoint

# Run the linter
lint:
    cargo clippy --all-targets --all-features
