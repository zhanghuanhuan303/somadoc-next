#!/bin/sh
# Render the full-capability sample: every engine feature in one book.
#
#   scripts/full.sh
#
# Writes examples/book/out/full.{pdf,html,json} and a pinyin-ordered index.
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

set -- --out "$out/full.smt" --face "serif:normal:$serif"
[ -f "$serif_bold" ] && set -- "$@" --face "serif:bold:$serif_bold"
[ -f "$sans" ] && set -- "$@" --face "sans:normal:$sans"
[ -f "$sans_bold" ] && set -- "$@" --face "sans:bold:$sans_bold"
[ -f "$rescue" ] && set -- "$@" --face "fallback:normal:$rescue"
[ -f "$math" ] && set -- "$@" --face "math:normal:$math"
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"

# The book template, with the index ordered by pinyin.
# A capability sheet reads better as one continuous flow: keep the book's
# page size, masters and numbering, but let a chapter start mid-page.
sed -e 's/^index:$/index:\n  sort: pinyin/' \
    -e 's/page-break-before: true/page-break-before: false/' \
    examples/book/song.yaml > "$out/full.yaml"

set -- --input examples/book/full.tex --template "$out/full.yaml" \
    --metrics "$out/full.smt" --hyphenation examples/book/hyphen-en.pat --stats

for format in html json; do
    cargo run --quiet -p soma-cli -- render "$@" --format "$format" \
        --out "$out/full.$format"
done

set -- "$@" --format pdf --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" --out "$out/full.pdf"

# A one-page supplement for the two HTML-only constructs: `<colgroup>`
# relative widths and native `<ruby>`.
set -- --input examples/book/full-html.html --template "$out/full.yaml" \
    --metrics "$out/full.smt" --format pdf --out "$out/full-html.pdf" \
    --font "$serif"
[ -f "$serif_bold" ] && set -- "$@" --font-bold "$serif_bold"
[ -f "$sans" ] && set -- "$@" --font-sans "$sans"
[ -f "$sans_bold" ] && set -- "$@" --font-sans-bold "$sans_bold"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render "$@" >/dev/null

echo "wrote $out/full.pdf and $out/full-html.pdf"
