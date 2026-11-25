#!/usr/bin/env bash
# Real-corpus evaluation and proxy calibration.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== corpus: per-document metrics (default-v1) =="
cargo run -q -p soma-eval --release -- corpus --dir corpus/zh

echo
echo "== corpus: tolerance x badness-scale sweep (proxy objective) =="
cargo run -q -p soma-eval --release -- calibrate --dir corpus/zh
