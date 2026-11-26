#!/bin/sh
# Assemble the static files browser-local mode expects under dist/.
#
#   bash scripts/local-assets.sh
#
# Needs `pyftsubset` (pip install fonttools brotli) and a font directory:
#   SOMA_FONT_DIR=/path/to/fonts bash scripts/local-assets.sh
#
# The proof uses subset WOFF2 (small first paint); PDF export fetches the full
# TTF/OTF files from fonts/v1/.
set -eu
cd "$(dirname "$0")/.."

DIST=crates/soma-studio-ui/dist
FONTS=${SOMA_FONT_DIR:-$HOME/.fonts}
PYFTSUBSET=${PYFTSUBSET:-pyftsubset}

if [ ! -f "$DIST/index.html" ]; then
    echo "dist/index.html is missing; run trunk build in crates/soma-studio-ui first" >&2
    exit 1
fi
if ! command -v "$PYFTSUBSET" >/dev/null 2>&1; then
    echo "pyftsubset not found; pip install fonttools brotli" >&2
    exit 1
fi

PLEX_REGULAR="$FONTS/IBMPlexSansSC_Regular.ttf"
PLEX_BOLD="$FONTS/IBMPlexSansSC_Bold.ttf"
MONO_REGULAR="$FONTS/NotoSansMono-Regular.ttf"
MONO_BOLD="$FONTS/NotoSansMono-Bold.ttf"
MATH="$FONTS/NewCMMath-Regular.otf"
for file in "$PLEX_REGULAR" "$PLEX_BOLD" "$MONO_REGULAR" "$MONO_BOLD" "$MATH"; do
    [ -f "$file" ] || { echo "missing $file (set SOMA_FONT_DIR)" >&2; exit 1; }
done

mkdir -p "$DIST/fonts/v1/subset" "$DIST/fonts/v1/pdf-subset" "$DIST/fig"
cp "$PLEX_REGULAR" "$DIST/fonts/v1/IBMPlexSansSC_Regular.ttf"
cp "$PLEX_BOLD" "$DIST/fonts/v1/IBMPlexSansSC_Bold.ttf"
cp "$MONO_REGULAR" "$DIST/fonts/v1/NotoSansMono-Regular.ttf"
cp "$MONO_BOLD" "$DIST/fonts/v1/NotoSansMono-Bold.ttf"
cp "$MATH" "$DIST/fonts/v1/NewCMMath-Regular.otf"
cp examples/book/fig/* "$DIST/fig/"

# Every character in the demo document plus the Studio's own labels.
python3 - <<'PY'
from pathlib import Path
chars = set(Path('examples/book/full.tex').read_text())
chars |= set(Path('examples/templates/plex.yaml').read_text())
chars |= set(Path('examples/templates/proposal.engine').read_text())
for path in Path('crates/soma-studio-ui/src').rglob('*.rs'):
    chars |= set(path.read_text(errors='ignore'))
for path in Path('crates/soma-studio-ui/style').glob('*.css'):
    chars |= set(path.read_text(errors='ignore'))
Path('/tmp/soma-subset.txt').write_text(''.join(sorted(chars)))
PY

COMMON='U+0020-007E,U+00A0-00FF,U+2000-206F,U+3000-303F,U+FF00-FFEF'
MATH_RANGES='U+0020-007E,U+00A0-00FF,U+0370-03FF,U+2000-206F,U+2190-21FF,U+2200-22FF,U+27C0-27EF,U+2980-29FF'
"$PYFTSUBSET" "$PLEX_REGULAR" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' --flavor=woff2 \
    --output-file="$DIST/fonts/v1/subset/IBMPlexSansSC_Regular.woff2" 2>/dev/null
"$PYFTSUBSET" "$PLEX_BOLD" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' --flavor=woff2 \
    --output-file="$DIST/fonts/v1/subset/IBMPlexSansSC_Bold.woff2" 2>/dev/null
"$PYFTSUBSET" "$MONO_REGULAR" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' --flavor=woff2 \
    --output-file="$DIST/fonts/v1/subset/NotoSansMono-Regular.woff2" 2>/dev/null
"$PYFTSUBSET" "$MONO_BOLD" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' --flavor=woff2 \
    --output-file="$DIST/fonts/v1/subset/NotoSansMono-Bold.woff2" 2>/dev/null
"$PYFTSUBSET" "$MATH" --text-file=/tmp/soma-subset.txt --unicodes="$MATH_RANGES" \
    --layout-features='*' --flavor=woff2 \
    --output-file="$DIST/fonts/v1/subset/NewCMMath-Regular.woff2" 2>/dev/null

# PDF subsets use the whole demo + template text. They are picked only when
# the current source is covered; otherwise the exporter fetches the full faces.
"$PYFTSUBSET" "$PLEX_REGULAR" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' \
    --output-file="$DIST/fonts/v1/pdf-subset/IBMPlexSansSC_Regular.ttf" 2>/dev/null
"$PYFTSUBSET" "$PLEX_BOLD" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' \
    --output-file="$DIST/fonts/v1/pdf-subset/IBMPlexSansSC_Bold.ttf" 2>/dev/null
"$PYFTSUBSET" "$MONO_REGULAR" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' \
    --output-file="$DIST/fonts/v1/pdf-subset/NotoSansMono-Regular.ttf" 2>/dev/null
"$PYFTSUBSET" "$MONO_BOLD" --text-file=/tmp/soma-subset.txt --unicodes="$COMMON" \
    --layout-features='*' \
    --output-file="$DIST/fonts/v1/pdf-subset/NotoSansMono-Bold.ttf" 2>/dev/null
"$PYFTSUBSET" "$MATH" --text-file=/tmp/soma-subset.txt --unicodes="$MATH_RANGES" \
    --layout-features='*' \
    --output-file="$DIST/fonts/v1/pdf-subset/NewCMMath-Regular.otf" 2>/dev/null

du -sh "$DIST/fonts/v1" "$DIST/fig"
ls -lh "$DIST/fonts/v1/subset" "$DIST/fonts/v1/pdf-subset"
