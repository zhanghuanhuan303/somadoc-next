#!/bin/sh
# Build a long-form sample: the corpus as three volumes plus a mixed-font page.
#
#   scripts/anthology.sh
#
# Writes examples/book/out/anthology.{md,pdf}. The Markdown is generated so the
# sample stays in sync with corpus/zh.
set -eu

cd "$(dirname "$0")/.."
out=examples/book/out
mkdir -p "$out"

# A body face plus a rescue face: MiSans for text, LXGW WenKai for the handful
# of glyphs MiSans lacks (the mixed-font page relies on it).
pdf_font=${SOMA_PDF_FONT:-/usr/share/fonts/misan/MiSans-Normal.ttf}
rescue=${SOMA_FALLBACK_FONT:-/usr/share/fonts/TTF/LXGWWenKai-Regular.ttf}
math=${SOMA_PDF_MATH:-/usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf}

{
    cat <<'FRONT'
---
title: 中文排版案例集
subtitle: 九篇真实语料，三卷排样
author: soma-typeset
date: 2026-11-20
language: zh-Hans
---

FRONT
    for volume in 一 二 三; do
        for file in corpus/zh/*.md; do
            sed "1s/\$/（卷$volume）/; 1s/（卷$volume） \$/（卷$volume）/" "$file"
            printf '\n\n'
        done
    done
    cat <<'MIXED'
# 混排演示

这一段用来检查回退字体。正文由 MiSans 度量并绘制，下面的生僻字它没有，需要由霞鹜文楷补上：𪚥、𪚥、𪚥。

回退不是事后修补：断行时用的就是回退字体的 advance 与左右边缘，所以这一行的末字仍然压在版心上。只有一个字体也没有的字才会显示为缺字符，例如 𠀀。

混排行与普通行的行盒取两者 ascent 与 descent 的较大值，因此行距不会因为一个高字形而跳动。
MIXED
} > "$out/anthology.md"

faces="--face serif:normal:$pdf_font"
[ -f "$rescue" ] && faces="$faces --face fallback:normal:$rescue"
[ -f "$math" ] && faces="$faces --face math:normal:$math"
# shellcheck disable=SC2086
cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- \
    --out "$out/anthology.smt" $faces

set -- --format pdf --font "$pdf_font" --out "$out/anthology.pdf"
[ -f "$rescue" ] && set -- "$@" --font-fallback "$rescue"
[ -f "$math" ] && set -- "$@" --font-math "$math"
cargo run --quiet -p soma-cli -- render \
    --input "$out/anthology.md" \
    --template examples/book/book.yaml \
    --metrics "$out/anthology.smt" \
    --stats "$@"

echo "wrote $out/anthology.pdf"
