#!/usr/bin/env bash
# Build mbexpr.wasm reproducibly: the toolchain is pinned in
# rust-toolchain.toml, the dependencies in Cargo.lock, and the local paths
# a panic location would carry (this checkout, the cargo registry) are
# remapped, so the same inputs give the same bytes on any machine.
# Prints the sha256 and size to pin in mechbench-compute.
set -euo pipefail
cd "$(dirname "$0")"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="--remap-path-prefix=$(pwd)=/mbexpr --remap-path-prefix=$CARGO_HOME=/cargo"
cargo build --locked --release --target wasm32-unknown-unknown
mkdir -p build
cp target/wasm32-unknown-unknown/release/mbexpr.wasm build/mbexpr.wasm
if command -v sha256sum >/dev/null; then sha256sum build/mbexpr.wasm; else shasum -a 256 build/mbexpr.wasm; fi
wc -c < build/mbexpr.wasm
