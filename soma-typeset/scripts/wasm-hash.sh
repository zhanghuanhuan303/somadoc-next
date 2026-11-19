#!/usr/bin/env bash
# Gate T: native vs wasm32-wasip1 canonical bytes must match exactly.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v node >/dev/null 2>&1; then
    echo "node is required for the wasm32-wasip1 hash gate" >&2
    exit 1
fi

mkdir -p target/wasm-hash

echo "== native fixture =="
cargo run -q -p soma-wasm --release > target/wasm-hash/native.json

echo "== wasm32-wasip1 build =="
cargo build -q -p soma-wasm --release --target wasm32-wasip1

echo "== wasm32-wasip1 run =="
node scripts/run-wasi.mjs target/wasm32-wasip1/release/soma-wasm.wasm \
    > target/wasm-hash/wasm.json

if ! cmp -s target/wasm-hash/native.json target/wasm-hash/wasm.json; then
    echo "native/wasm canonical bytes differ" >&2
    diff -u target/wasm-hash/native.json target/wasm-hash/wasm.json || true
    exit 1
fi

bytes="$(wc -c < target/wasm-hash/native.json | tr -d ' ')"
printf 'native/wasm canonical bytes match (%s bytes)\n' "$bytes"
