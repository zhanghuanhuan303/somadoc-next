#!/bin/sh
# Assemble the static fonts browser-local mode expects under dist/fonts/v2.
#
#   SOMA_FONT_DIR=/path/to/fonts bash scripts/local-assets.sh
#
# Requires `pyftsubset` (pip install fonttools). The CJK faces are GB2312
# levels 1+2 (6 763 characters), following the huans somadoc lab: the proof and
# PDF share the same files, so exporting does not download a second face and no
# coverage check is needed. Characters outside GB2312 render as empty boxes.
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
    echo "pyftsubset not found; pip install fonttools" >&2
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

mkdir -p "$DIST/fonts/v2"

# GB2312 levels 1+2, the CJK punctuation blocks, ASCII and the characters the
# template/lab copy needs. This is the lab's `subset-cjk.py` charset.
python3 - <<'PY'
from pathlib import Path
chars = set()
for hi in range(0xB0, 0xF8):
    for lo in range(0xA1, 0xFF):
        try:
            chars.add(bytes([hi, lo]).decode('gb2312'))
        except UnicodeDecodeError:
            continue
chars.update(chr(cp) for cp in range(0x20, 0x7F))
chars.update(chr(cp) for cp in range(0x3000, 0x3040))
chars.update(chr(cp) for cp in range(0xFF00, 0xFFF0))
chars.update('–—‘’“”…·×÷±≈≤≥≠→←↑↓∞∑∫√°′″€£¥©®™§¶•◦')
chars.update('熵侘寂拓扑网格排版')
Path('/tmp/soma-gb2312.txt').write_text(''.join(sorted(chars)), encoding='utf-8')
print(f'charset: {len(chars)} characters')
PY

CJK='--text-file=/tmp/soma-gb2312.txt'
SUBSET='--layout-features=kern,liga,vert,vrt2,palt,halt --name-IDs=* --name-legacy --name-languages=* --drop-tables+=DSIG --no-hinting --recalc-bounds --recalc-average-width --notdef-outline'
LATIN='U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-205E,U+20AC,U+2116,U+2122,U+2212,U+2260,U+2264,U+2265'

# shellcheck disable=SC2086
"$PYFTSUBSET" "$PLEX_REGULAR" $CJK $SUBSET --output-file="$DIST/fonts/v2/IBMPlexSansSC_Regular.ttf"
# shellcheck disable=SC2086
"$PYFTSUBSET" "$PLEX_BOLD" $CJK $SUBSET --output-file="$DIST/fonts/v2/IBMPlexSansSC_Bold.ttf"
# shellcheck disable=SC2086
"$PYFTSUBSET" "$MONO_REGULAR" --unicodes="$LATIN" $SUBSET --output-file="$DIST/fonts/v2/NotoSansMono-Regular.ttf"
# shellcheck disable=SC2086
"$PYFTSUBSET" "$MONO_BOLD" --unicodes="$LATIN" $SUBSET --output-file="$DIST/fonts/v2/NotoSansMono-Bold.ttf"
cp "$MATH" "$DIST/fonts/v2/NewCMMath-Regular.otf"

du -sh "$DIST/fonts/v2"
ls -lh "$DIST/fonts/v2"
