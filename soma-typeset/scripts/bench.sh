#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
cargo run -p soma-eval --release -- bench --repeats 5 --check "$@"
