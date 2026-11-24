#!/usr/bin/env bash
# Gate Q': effect scorecard. Structural invariants are tests; the numbers are a
# report (a quality claim needs Gate Q's real corpus and reviewers).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate Q': effect scorecard tests =="
cargo test -q -p soma-eval --test effect

echo "== gate Q': real corpus tests =="
cargo test -q -p soma-eval --test corpus

echo "== gate Q': scorecard (40 paragraphs) =="
scorecard="$(cargo run -q -p soma-eval --release -- score --paragraphs 40)"
printf '%s\n' "$scorecard"
bdev="$(printf '%s\n' "$scorecard" | awk -F, '$1=="bdev_rel_error_p95"{print $2}')"
if [ -n "$bdev" ] && [ "$bdev" -gt 20 ]; then
    echo "boundary-deviation approximation error is ${bdev}‰ (threshold 20‰);" >&2
    echo "the proportional-allocation assumption no longer matches the renderer" >&2
    exit 1
fi

echo "== gate Q': real corpus (corpus/zh) =="
cargo run -q -p soma-eval --release -- corpus --dir corpus/zh
