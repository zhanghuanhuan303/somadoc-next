#!/usr/bin/env bash
# Gate V (visual regression) and Gate R (cross-renderer parity).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate V: SVG visual lock =="
cargo test -q -p soma-render-svg --test gate_r svg_bytes_are_a_visual_regression_lock

echo "== gate R: SVG paints the layout; HTML covers the same atoms =="
cargo test -q -p soma-render-svg --test gate_r

echo "gate V/R passed"
