# Common commands. Run `just` to list.

default:
    @just --list

# Run all Rust tests
test:
    cargo test

# Run the server with the local config
run:
    cargo run

# Release build
build:
    cargo build --release

# Rebuild the frontend bundle into static/
frontend:
    cd frontend && npm ci && npm run build

# Format + lint
check:
    cargo fmt --check
    cargo clippy -- -D warnings

fmt:
    cargo fmt

# Nix build of the packaged binary
nix-build:
    nix build
