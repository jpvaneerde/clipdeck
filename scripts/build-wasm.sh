#!/usr/bin/env bash
# Build the audio engine to WebAssembly and put it where the web app loads it.
set -euo pipefail
cd "$(dirname "$0")/.."
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build -p clipdeck-engine-wasm --lib --target wasm32-unknown-unknown --profile wasm
cp target/wasm32-unknown-unknown/wasm/clipdeck_engine_wasm.wasm web/public/engine.wasm
echo "web/public/engine.wasm ($(wc -c < web/public/engine.wasm) bytes)"
