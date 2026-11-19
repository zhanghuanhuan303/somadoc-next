#!/usr/bin/env bash
# Gate C (content): capability matrix, degradation promise, renderer safety.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate C: capability matrix matches the code =="
cargo test -q -p soma-md --test capabilities

echo "== gate C: the renderer emits no active content =="
cargo test -q -p soma-render-html --test safety

echo "== gate C: the matrix is documented =="
test -f docs/capabilities.md
grep -q 'Gate C' docs/capabilities.md

echo "gate C passed"
