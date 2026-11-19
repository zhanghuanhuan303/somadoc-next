#!/usr/bin/env bash
# Gate Q': effect scorecard. Structural invariants are tests; the numbers are a
# report (a quality claim needs Gate Q's real corpus and reviewers).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate Q': effect scorecard tests =="
cargo test -q -p soma-eval --test effect

echo "== gate Q': scorecard (40 paragraphs) =="
cargo run -q -p soma-eval --release -- score --paragraphs 40
