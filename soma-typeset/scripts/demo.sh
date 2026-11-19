#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
mkdir -p target
cargo run -p soma-demo --release -- "$@" > target/demo.html
printf 'wrote %s\n' "$(pwd)/target/demo.html"
