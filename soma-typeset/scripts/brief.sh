#!/bin/sh
# Render the delivery brief: same faces as the Song sample, same template.
#
#   scripts/brief.sh
#
# Writes examples/book/out/brief.{pdf,html,svg,json}. The metrics table is
# generated next to the other samples so the brief measures with exactly the
# faces it draws.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

serif=${SOMA_SONG_SERIF:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Regular.otf}
serif_bold=${SOMA_SONG_SERIF_BOLD:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Bold.otf}
sans=${SOMA_SONG_SANS:-/usr/share/fonts/misan/MiSans-Normal.ttf}
sans_bold=${SOMA_SONG_SANS_BOLD:-/usr/share/fonts/misan/MiSans-Bold.ttf}
rescue=${SOMA_SONG_FALLBACK:-/home/badleric/.local/share/fonts/somadoc-fandol/FandolKai-Regular.otf}
math=${SOMA_SONG_MATH:-$(sh scripts/math-face.sh)}

if [ ! -f "$serif" ]; then
    echo "no serif face at $serif; set SOMA_SONG_SERIF" >&2
    exit 1
fi

set -- --out "$out/brief.smt" --face "serif:normal:$serif"
[ -f "$serif_bold" ] && set -- "$@" --face "serif:bold:$serif_bold"
[ -f "$sans" ] && set -- "$@" --face "sans:normal:$sans"
[ -f "$sans_bold" ] && set -- "$@" --face "sans:bold:$sans_bold"
[ -f "$rescue" ] && set -- "$@" --face "fallback:normal:$rescue"
[ -f "$math" ] && set -- "$@" --face "math:normal:$math"
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"

set -- --input examples/book/brief.tex --template examples/book/song.yaml \
    --metrics "$out/brief.smt" --stats

for format in html svg json; do
    cargo run --quiet -p soma-cli -- render "$@" --format "$format" \
        --out "$out/brief.$format"
done

set -- "$@" --format pdf --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" --out "$out/brief.pdf"

echo "wrote $out/brief.pdf"
