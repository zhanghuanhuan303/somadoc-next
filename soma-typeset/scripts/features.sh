#!/bin/sh
# Render the feature sheet: ruby, hyphenation, nested lists, GFM tables, and a
# pinyin index from Markdown, with the same faces the book sample uses.
#
#   scripts/features.sh
#
# Writes examples/book/out/features.{pdf,html,json}.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

serif=${SOMA_SONG_SERIF:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Regular.otf}
serif_bold=${SOMA_SONG_SERIF_BOLD:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Bold.otf}
sans=${SOMA_SONG_SANS:-/usr/share/fonts/misan/MiSans-Normal.ttf}
sans_bold=${SOMA_SONG_SANS_BOLD:-/usr/share/fonts/misan/MiSans-Bold.ttf}
rescue=${SOMA_SONG_FALLBACK:-}
math=${SOMA_SONG_MATH:-$(sh scripts/math-face.sh)}

if [ ! -f "$serif" ]; then
    echo "no serif face at $serif; set SOMA_SONG_SERIF" >&2
    exit 1
fi

set -- --out "$out/features.smt" --face "serif:normal:$serif"
[ -f "$serif_bold" ] && set -- "$@" --face "serif:bold:$serif_bold"
[ -f "$sans" ] && set -- "$@" --face "sans:normal:$sans"
[ -f "$sans_bold" ] && set -- "$@" --face "sans:bold:$sans_bold"
[ -f "$rescue" ] && set -- "$@" --face "fallback:normal:$rescue"
[ -f "$math" ] && set -- "$@" --face "math:normal:$math"
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"

# The index asks for pinyin order; the rest of the template is the book sample.
sed 's/^index:$/index:\n  sort: pinyin/' examples/book/song.yaml > "$out/features.yaml"

set -- --input examples/book/features.md --template "$out/features.yaml" \
    --metrics "$out/features.smt" --hyphenation examples/book/hyphen-en.pat --stats

for format in html json; do
    cargo run --quiet -p soma-cli -- render "$@" --format "$format" \
        --out "$out/features.$format"
done

set -- "$@" --format pdf --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" --out "$out/features.pdf"

echo "wrote $out/features.pdf"
