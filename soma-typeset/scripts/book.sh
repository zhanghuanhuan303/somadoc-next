#!/bin/sh
# Render the first-release example book: cover, contents, chapters.
#
#   scripts/book.sh
#
# Writes examples/book/out/book.{html,svg,json,pdf}. The output is not
# committed; rerun this after changing the engine, the template, or the source.
#
# When a TrueType font is available every format is laid out with a metrics
# table generated from that same face, so HTML, SVG, JSON and PDF share one
# set of coordinates and the same punctuation ink (ADR-0037). Without a font
# the embedded table is used.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

# The PDF backend needs a TrueType (glyf) font. Override with SOMA_PDF_FONT /
# SOMA_PDF_MATH.
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
pdf_math=${SOMA_PDF_MATH:-$(sh scripts/math-face.sh)}

pdf_metrics=${SOMA_PDF_METRICS:-}
if [ -z "$pdf_metrics" ] && [ -n "$pdf_font" ] && [ -f "$pdf_font" ]; then
    # Layout must measure with the same face the PDF draws.
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

# Without a TrueType font the PDF is skipped, but the other formats can still
# reproduce the committed layout: measure with the OFL baseline table.
if [ -z "$pdf_metrics" ] && [ -f "examples/book/metrics-lxgw.smt" ]; then
    pdf_metrics="examples/book/metrics-lxgw.smt"
fi

metrics_opt=""
if [ -n "$pdf_metrics" ] && [ -f "$pdf_metrics" ]; then
    metrics_opt="--metrics $pdf_metrics"
fi

cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    $metrics_opt \
    --format html --out "$out/book.html" --stats
cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    $metrics_opt \
    --format svg --out "$out/book.svg"
cargo run --quiet -p soma-cli -- render \
    --input examples/book/book.tex \
    --template examples/book/book.yaml \
    $metrics_opt \
    --format json --out "$out/book.json"

echo "wrote $out/book.html, book.svg, book.json"

if [ -n "$pdf_font" ] && [ -f "$pdf_font" ]; then
    set -- --format pdf --font "$pdf_font" --out "$out/book.pdf"
    if [ -n "$pdf_bold" ] && [ -f "$pdf_bold" ]; then
        set -- "$@" --font-bold "$pdf_bold"
    fi
    if [ -f "$pdf_math" ]; then
        set -- "$@" --font-math "$pdf_math"
    fi
    cargo run --quiet -p soma-cli -- render \
        --input examples/book/book.tex \
        --template examples/book/book.yaml \
        $metrics_opt "$@"
    echo "wrote $out/book.pdf"
else
    echo "skipped PDF (set SOMA_PDF_FONT to a TrueType font)" >&2
fi
