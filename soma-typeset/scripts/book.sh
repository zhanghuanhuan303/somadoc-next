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

# PDF: needs a TrueType (glyf) font. Override with SOMA_PDF_FONT / SOMA_PDF_MATH.
pdf_font=${SOMA_PDF_FONT:-}
if [ -z "$pdf_font" ]; then
    for candidate in \
        /usr/share/fonts/misan/MiSans-Normal.ttf \
        /home/badleric/.fonts/IBMPlexSansSC_Text.ttf \
        /usr/share/fonts/TTF/LXGWWenKai-Regular.ttf; do
        if [ -f "$candidate" ]; then
            pdf_font=$candidate
            break
        fi
    done
fi
pdf_bold=${SOMA_PDF_FONT_BOLD:-}
pdf_math=${SOMA_PDF_MATH:-/usr/share/fonts/noto/NotoSansMath-Regular.ttf}
if [ -n "$pdf_font" ] && [ -f "$pdf_font" ]; then
    # Layout must measure with the same face the PDF draws. Generate a table
    # from the PDF font (and math face) unless the caller provides one.
    pdf_metrics=${SOMA_PDF_METRICS:-}
    if [ -z "$pdf_metrics" ]; then
        pdf_metrics="$out/metrics-pdf.smt"
        set -- --out "$pdf_metrics" --face "serif:normal:$pdf_font"
        if [ -n "$pdf_bold" ] && [ -f "$pdf_bold" ]; then
            set -- "$@" --face "serif:bold:$pdf_bold"
        fi
        if [ -f "$pdf_math" ]; then
            set -- "$@" --face "math:normal:$pdf_math"
        fi
        cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"
    fi

    set -- --format pdf --font "$pdf_font" --metrics "$pdf_metrics" --out "$out/book.pdf"
    if [ -n "$pdf_bold" ] && [ -f "$pdf_bold" ]; then
        set -- "$@" --font-bold "$pdf_bold"
    fi
    if [ -f "$pdf_math" ]; then
        set -- "$@" --font-math "$pdf_math"
    fi
    cargo run --quiet -p soma-cli -- render \
        --input examples/book/book.tex \
        --template examples/book/book.yaml "$@"
    echo "wrote $out/book.pdf"
else
    echo "skipped PDF (set SOMA_PDF_FONT to a TrueType font)" >&2
fi
