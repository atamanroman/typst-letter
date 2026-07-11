[private]
default:
    @just --list

# Run the server with the local config
run:
    cargo run

# Watch src/ + frontend/, rebuild bundle and restart server on change
dev:
    #!/usr/bin/env bash
    set -m
    (cd frontend && npx esbuild src/editor.js --bundle --format=iife --outfile=../static/editor.js --watch) &
    trap 'kill %1 2>/dev/null' EXIT
    watchexec --restart --watch src --watch static --watch Cargo.toml -- cargo run

# Run all Rust tests
test:
    cargo test

# Release build
build:
    cargo build --release

# Rebuild the minified frontend bundle into static/
frontend:
    cd frontend && npm ci && npm run build

# Nix build of the packaged binary
nix-build:
    nix build
