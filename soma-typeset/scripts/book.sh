#!/bin/sh
# Render the first-release example book: cover, contents, chapters.
#
#   scripts/book.sh
#
# Writes examples/book/out/book.{html,svg,json}. The output is not committed;
# rerun this after changing the engine, the template, or the source.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    --format html --out "$out/book.html" --stats
cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    --format svg --out "$out/book.svg"
cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    --format json --out "$out/book.json"

echo "wrote $out/book.html, book.svg, book.json"
