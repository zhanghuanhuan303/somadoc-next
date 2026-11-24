#!/bin/sh
# Render the Song-typeface sample: 宋体 body, 黑体 headings, 楷体 rescue.
#
#   scripts/song.sh
#
# Writes examples/book/out/song.{pdf,html,svg,json} and the metrics table the
# layout measures with. Every face is optional: a missing one simply removes
# that family from the output (Zed: the PDF backend embeds CFF and TrueType
# alike, so 思源宋体 and MiSans can share one document).
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

# Layout must measure with the same faces the PDF draws.
set -- --out "$out/song.smt" --face "serif:normal:$serif"
[ -f "$serif_bold" ] && set -- "$@" --face "serif:bold:$serif_bold"
[ -f "$sans" ] && set -- "$@" --face "sans:normal:$sans"
[ -f "$sans_bold" ] && set -- "$@" --face "sans:bold:$sans_bold"
[ -f "$rescue" ] && set -- "$@" --face "fallback:normal:$rescue"
[ -f "$math" ] && set -- "$@" --face "math:normal:$math"
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"

set -- --input examples/book/song.tex --template examples/book/song.yaml \
    --metrics "$out/song.smt" --stats

# Every format gets the same asset root, so figures resolve for all of them.
for format in html svg json; do
    cargo run --quiet -p soma-cli -- render "$@" --format "$format" \
        --out "$out/song.$format"
done

set -- "$@" --format pdf --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" --out "$out/song.pdf"

echo "wrote $out/song.pdf"
