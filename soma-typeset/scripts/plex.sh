#!/bin/sh
# Render the reference page design from examples/templates/plex.yaml and the
# multi-language code-block sample from examples/templates/code.md.
#
#   scripts/plex.sh
#
# The design is transcribed from pandoc's `soma.latex`: IBM Plex Sans SC for
# every family, A4 with one-inch margins, 10pt on a 14.4pt baseline, no
# first-line indent, a 20pt paragraph skip, italic quotations, and booktabs'
# three-rule tables. The metrics table is generated from the same faces the PDF
# draws, so layout coordinates and punctuation ink agree (ADR-0037).
set -eu

cd "$(dirname "$0")/.."
out=examples/templates/out
mkdir -p "$out"

# IBM Plex Sans SC is not redistributed. Use the system copy, or point the two
# variables at a local install.
regular=${SOMA_PLEX_REGULAR:-}
if [ -z "$regular" ]; then
    regular=$(fc-match -f '%{file}' 'IBM Plex Sans SC' 2>/dev/null || true)
fi
bold=${SOMA_PLEX_BOLD:-}
if [ -z "$bold" ]; then
    bold=$(fc-match -f '%{file}' 'IBM Plex Sans SC:style=Bold' 2>/dev/null || true)
fi
math=${SOMA_PDF_MATH:-$(sh scripts/math-face.sh)}
mono=${SOMA_PLEX_MONO:-}
if [ -z "$mono" ]; then
    mono=$(fc-match -f '%{file}' 'Noto Sans Mono' 2>/dev/null || true)
fi
mono_bold=${SOMA_PLEX_MONO_BOLD:-}
if [ -z "$mono_bold" ]; then
    mono_bold=$(fc-match -f '%{file}' 'Noto Sans Mono:style=Bold' 2>/dev/null || true)
fi

if [ -z "$regular" ] || [ ! -f "$regular" ]; then
    echo "missing IBM Plex Sans SC; set SOMA_PLEX_REGULAR and SOMA_PLEX_BOLD" >&2
    exit 1
fi
if [ -z "$bold" ] || [ ! -f "$bold" ]; then
    echo "missing IBM Plex Sans SC Bold; set SOMA_PLEX_BOLD" >&2
    exit 1
fi
if [ -z "$mono" ] || [ ! -f "$mono" ]; then
    echo "missing a monospaced face; set SOMA_PLEX_MONO (e.g. Noto Sans Mono)" >&2
    exit 1
fi
if [ -z "$mono_bold" ] || [ ! -f "$mono_bold" ]; then
    mono_bold=$mono
fi

input=${SOMA_INPUT:-examples/book/full.tex}
# The Studio uses the committed `examples/templates/plex.smt`; this sample
# regenerates its own ignored copy so running it does not dirty the tree.
metrics="$out/plex.smt"

# Register each face under both family slots: the reference maps \setmainfont,
# \setsansfont and \setmonofont to the same design. The Plex template now maps
# code to a real monospace face (ADR-0089); CJK inside a code run is measured
# by the fallback face and drawn with the Plex fallback the PDF embeds.
#
# IBM Plex Sans SC declares a one-em lineGap. The reference's \linespread{1.2}
# sets the baseline; the template's line-height does the same here, so ignore
# the font's line recommendation.
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- \
    --out "$metrics" \
    --leading 0 \
    --face "serif:normal:$regular" \
    --face "serif:bold:$bold" \
    --face "sans:normal:$regular" \
    --face "sans:bold:$bold" \
    --face "mono:normal:$mono" \
    --face "mono:bold:$mono_bold" \
    --face "fallback:normal:$regular" \
    --face "math:normal:$math"

cargo run --quiet -p soma-cli -- render \
    --input "$input" \
    --template examples/templates/plex.yaml \
    --metrics "$metrics" \
    --format pdf \
    --font "$regular" \
    --font-bold "$bold" \
    --font-sans "$regular" \
    --font-sans-bold "$bold" \
    --font-mono "$mono" \
    --font-mono-bold "$mono_bold" \
    --font-fallback "$regular" \
    --font-math "$math" \
    --out "$out/plex.pdf"

echo "wrote $out/plex.pdf"

# The code-block sample exercises every language profile and the panel in one
# file; it uses the same faces and metrics as the reference page.
cargo run --quiet -p soma-cli -- render \
    --input examples/templates/code.md \
    --template examples/templates/plex.yaml \
    --metrics "$metrics" \
    --format pdf \
    --font "$regular" \
    --font-bold "$bold" \
    --font-sans "$regular" \
    --font-sans-bold "$bold" \
    --font-mono "$mono" \
    --font-mono-bold "$mono_bold" \
    --font-fallback "$regular" \
    --font-math "$math" \
    --out "$out/code.pdf"

echo "wrote $out/code.pdf"

# The TikZ sample pairs with examples/tikz/figure.tex, whose pdflatex output is
# the visual reference (ADR-0092).
cargo run --quiet -p soma-cli -- render \
    --input examples/tikz/figure-soma.tex \
    --template examples/templates/plex.yaml \
    --metrics "$metrics" \
    --format pdf \
    --font "$regular" \
    --font-bold "$bold" \
    --font-sans "$regular" \
    --font-sans-bold "$bold" \
    --font-mono "$mono" \
    --font-mono-bold "$mono_bold" \
    --font-fallback "$regular" \
    --font-math "$math" \
    --out "$out/tikz-sample.pdf"

echo "wrote $out/tikz-sample.pdf"
