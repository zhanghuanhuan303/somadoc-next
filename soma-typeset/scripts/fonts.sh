#!/bin/sh
# Pick the best usable body font on this machine and write a metrics table.
#
#   scripts/fonts.sh [out.smt]
#
# Scans the usual font directories, scores every face with
# `metrics-gen --audit`, and writes a `soma.metrics/2` table for the
# highest-scoring face in an allow-list of families whose licence permits
# shipping the derived measurements. Falls back to any usable face, then to the
# committed baseline.
set -eu

cd "$(dirname "$0")/.."
out=${1:-examples/book/metrics-auto.smt}

audit_args=""
for dir in /usr/share/fonts /usr/local/share/fonts "$HOME/.fonts" "$HOME/.local/share/fonts"; do
    if [ -d "$dir" ]; then
        audit_args="$audit_args --audit $dir"
    fi
done

if [ -z "$audit_args" ]; then
    echo "fonts.sh: no font directory found" >&2
    exit 1
fi

report=$(cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- \
    $audit_args --top 4000 2>/dev/null)

# One face from the report, filtered by family pattern and file-name pattern.
find_face() {
    printf '%s\n' "$report" |
        awk -F, -v allow="$1" -v want="$2" '
            NR > 1 && $2 == "usable" && $3 ~ allow && (want == "" || $3 ~ want) {
                print $3 "#" $4
                exit
            }'
}

# OFL first, then GPL-with-exception, then whatever is usable. Inside a family a
# Regular cut is preferred: a Light or Thin face would quietly set the whole
# document lighter than intended.
pick_face=""
for allow in \
    'LXGWWenKai|IBMPlex|NotoSansCJK|NotoSerifCJK|SourceHan|NotoSansSC|NotoSerifSC' \
    'wqy|WenQuanYi' \
    ''; do
    pick_face=$(find_face "$allow" '(Regular|Normal)')
    [ -n "$pick_face" ] && break
    pick_face=$(find_face "$allow" '')
    [ -n "$pick_face" ] && break
done
if [ -z "$pick_face" ]; then
    echo "fonts.sh: no usable font on this machine" >&2
    exit 1
fi

pick_path=${pick_face%%#*}
pick_index=${pick_face##*#}
echo "fonts.sh: picked $pick_path face $pick_index"

# A bold companion in the same family, if the naming follows the usual
# convention. Anything else falls back to one face.
bold_path=$(printf '%s\n' "$pick_path" | sed -E 's/([-_])(Regular|Normal|Book|Text)([.]ttf|[.]ttc|[.]otf)$/\1Bold\3/')
if [ "$bold_path" = "$pick_path" ] || [ ! -f "$bold_path" ]; then
    bold_path=""
fi

set -- --out "$out" --face "serif:normal:$pick_path#$pick_index"
if [ -n "$bold_path" ]; then
    set -- "$@" --face "serif:bold:$bold_path#$pick_index"
fi
# A serif math face first: it matches a Chinese serif body and, being `glyf`
# based, lets metrics-gen record per-glyph vertical ink (a CFF math face keeps
# the MATH constants but no ink).
for math in /usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf \
    /usr/share/fonts/libertinus/LibertinusMath-Regular.ttf \
    /usr/share/fonts/noto/NotoSansMath-Regular.ttf; do
    if [ -f "$math" ]; then
        set -- "$@" --face "math:normal:$math"
        break
    fi
done

cargo run --quiet --manifest-path tools/metrics-gen/Cargo.toml -- "$@"
echo "fonts.sh: wrote $out"
