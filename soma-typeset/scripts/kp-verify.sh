#!/usr/bin/env bash
# K-P closure verification. Run after the architecture freeze.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== KP closure: repository gates =="
bash scripts/gates.sh

echo "== KP closure: native benchmark (budget check) =="
bash scripts/bench.sh

echo "== KP closure: deterministic length policy =="
cargo run -q -p soma-eval --release -- \
    bench --repeats 3 --sizes 2000,4000 --max-kp-atoms 2000 --check
