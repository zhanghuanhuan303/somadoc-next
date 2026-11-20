#!/bin/sh
# Render the Math v2 showcase: TeX atom classes and spacing, style levels,
# ink-based fractions/radicals/fences, accents, inline breaking.
#
#   scripts/math.sh
#
# Writes examples/book/out/math-v2.pdf. The metrics table is generated next to
# it with the same faces, so the demo measures exactly what it draws.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

serif=${SOMA_SONG_SERIF:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Regular.otf}
serif_bold=${SOMA_SONG_SERIF_BOLD:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Bold.otf}
sans=${SOMA_SONG_SANS:-/usr/share/fonts/misan/MiSans-Normal.ttf}
sans_bold=${SOMA_SONG_SANS_BOLD:-/usr/share/fonts/misan/MiSans-Bold.ttf}
rescue=${SOMA_SONG_FALLBACK:-/home/badleric/.local/share/fonts/somadoc-fandol/FandolKai-Regular.otf}
math=${SOMA_SONG_MATH:-/usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf}

if [ ! -f "$serif" ]; then
    echo "no serif face at $serif; set SOMA_SONG_SERIF" >&2
    exit 1
fi
if [ ! -f "$math" ]; then
    echo "no math face at $math; set SOMA_SONG_MATH" >&2
    exit 1
fi

set -- --out "$out/math-v2.smt" --face "serif:normal:$serif"
[ -f "$serif_bold" ] && set -- "$@" --face "serif:bold:$serif_bold"
[ -f "$sans" ] && set -- "$@" --face "sans:normal:$sans"
[ -f "$sans_bold" ] && set -- "$@" --face "sans:bold:$sans_bold"
[ -f "$rescue" ] && set -- "$@" --face "fallback:normal:$rescue"
set -- "$@" --face "math:normal:$math"
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"

set -- --input examples/book/math-demo.tex --metrics "$out/math-v2.smt" --stats
set -- "$@" --format pdf --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" --out "$out/math-v2.pdf"

echo "wrote $out/math-v2.pdf"
