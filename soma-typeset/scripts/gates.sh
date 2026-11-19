#!/usr/bin/env bash
# Repository gates. Run this before every K1 red/green checkpoint and in CI.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate: format =="
cargo fmt --all --check

echo "== gate: clippy =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== gate: tests =="
cargo test --workspace

echo "== gate: wasm32 check =="
cargo check -p soma-core -p soma-doc -p soma-template -p soma-layout -p soma-md -p soma-render-html --target wasm32-unknown-unknown

echo "== gate: wasm32-wasip1 hash =="
bash scripts/wasm-hash.sh

echo "== gate: demo render =="
demo_file="$(mktemp)"
cargo run -q -p soma-demo --release > "$demo_file"
test -s "$demo_file"
grep -q 'data-algorithm="optimal"' "$demo_file"
rm -f "$demo_file"

echo "== gate: soma-core dependency boundary =="
if cargo tree -p soma-core | grep -Eq 'typst|fontdb|ttf-parser|wasm-bindgen|comrak|serde_json'; then
    echo "dependency boundary violated: soma-core must stay runtime-independent" >&2
    exit 1
fi

echo "== gate: clean-room references =="
if grep -rn "somadoc-engine" README.md PROVENANCE.md crates docs \
    --include='*.md' --include='*.rs' | grep -v 'docs/archive/'; then
    echo "clean-room violation: active files must not reference the deleted prior project" >&2
    exit 1
fi

echo "== gate: architecture (Gate A) =="
bash scripts/gate-a.sh

echo "all gates passed"
