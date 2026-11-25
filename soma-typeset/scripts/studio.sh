#!/bin/sh
# Gate S: the all-Rust Studio (S1-S11).
#
#   S1 shell baseline      38/34/29/24, three 34px bars share a baseline, one tree
#   S2 tabs                three files, no duplicates, right-neighbour close, empty workspace
#   S3 panels              sidebar 180-380 / AI 320-900, double-click reset, collapse
#   S4 theme               first-paint theme, persistence, both toggle paths
#   S5 proof box tree      page slots == fragments == pages; shadow page per slot; PDF page count
#   S6 live                debounced rebuild on input; IME composition stays quiet
#   S7 inspect             row count == overlay lines; click selects the same box
#   S8 offline/security    no external requests, no script/a[href] in pages, CSP header
#   S9 fonts/rebuild       probe p95, missing fonts degrade, SSE built/error, old build kept
#   S10 AI boundary        remote off; local reply is text-only; no engine imports in ai.rs
#   S11 mixed frontends    a tab parses with its own format (md/html/tex) in wasm and native
#
# The browser assertions live in `soma-studio-ui/src/selftest.rs` and are read
# from `#selftest` with `--dump-dom`. Requires chromium; trunk is used when
# present unless SOMA_STUDIO_SKIP_TRUNK=1.
set -eu

cd "$(dirname "$0")/.."

PORT="${SOMA_STUDIO_PORT:-7091}"
FONTLESS_PORT=$((PORT + 1))
WORK="$(mktemp -d "${TMPDIR:-/tmp}/soma-studio-gate.XXXXXX")"
EVENTS="$WORK/events.txt"
PID=""
FONTLESS_PID=""
CURL_PID=""
CHROME_DIR="$WORK/chrome"
FAILED=0

PLEX_REGULAR="${SOMA_PLEX_REGULAR:-$HOME/.fonts/IBMPlexSansSC_Regular.ttf}"
PLEX_BOLD="${SOMA_PLEX_BOLD:-$HOME/.fonts/IBMPlexSansSC_Bold.ttf}"
MONO_REGULAR="${SOMA_MONO_REGULAR:-/usr/share/fonts/noto/NotoSansMono-Regular.ttf}"
MONO_BOLD="${SOMA_MONO_BOLD:-/usr/share/fonts/noto/NotoSansMono-Bold.ttf}"
MATH_FACE="${SOMA_MATH_FACE:-/usr/share/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf}"
METRICS="${SOMA_METRICS:-examples/templates/out/plex.smt}"

cleanup() {
    [ -n "$CURL_PID" ] && kill "$CURL_PID" 2>/dev/null || true
    [ -n "$PID" ] && kill "$PID" 2>/dev/null || true
    [ -n "$FONTLESS_PID" ] && kill "$FONTLESS_PID" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

fail() {
    echo "S-gate: $1" >&2
    FAILED=1
}

echo "== gate S: build =="
if [ "${SOMA_STUDIO_SKIP_TRUNK:-0}" != "1" ] && command -v trunk >/dev/null 2>&1; then
    (cd crates/soma-studio-ui && trunk build --release >/dev/null)
else
    echo "using existing crates/soma-studio-ui/dist"
fi
if [ ! -f crates/soma-studio-ui/dist/index.html ]; then
    echo "crates/soma-studio-ui/dist is missing; run trunk build" >&2
    exit 1
fi

echo "== gate S: fixtures =="
cp examples/templates/plex.yaml "$WORK/template.yaml"
cp examples/book/hyphen-en.pat "$WORK/hyphen-en.pat"
cat > "$WORK/main.md" <<'EOF'
# 主文档

这是主文档的第一段，用来验证校样、检视与 wasm/native 哈希。中文排版需要处理标点挤压、行边裁剪与中英边界，Latin words stay on the baseline.

## 第二节

这是一个足够长的段落，用来让布局产生多行与至少一页。断行、分页、页眉页脚与索引都由同一个引擎在浏览器里完成，服务端只负责文件、字体与 PDF。

- 列表项一
- 列表项二

> 引用段落：确定性来自整数定点与固定迭代顺序。

```rust
fn main() {
    println!("hello soma");
}
```

| 列 A | 列 B |
| --- | --- |
| 1 | 2 |

这是最后一段，包含**加粗**、*斜体*、`行内代码`与[链接](https://example.com)。
EOF
printf '# 文档 B\n\nB 文档的正文，用于标签页与不重复打开。\n' > "$WORK/b.md"
printf '# 文档 C\n\nC 文档的正文，用于关闭活动标签后选择右邻。\n' > "$WORK/c.md"
cat > "$WORK/extra.html" <<'EOF'
<!doctype html>
<html><body>
<h1>HTML 标记</h1>
<p>网页标记进入 DocIR 后与 Markdown 走同一套断行。</p>
</body></html>
EOF
cat > "$WORK/extra.tex" <<'EOF'
\documentclass{article}
\begin{document}
\section{源码标题}
源码命令进入 DocIR 后与 Markdown 走同一套断行。
\end{document}
EOF

FONT_ARGS=""
if [ -f "$PLEX_REGULAR" ]; then
    FONT_ARGS="--font $PLEX_REGULAR"
    [ -f "$PLEX_BOLD" ] && FONT_ARGS="$FONT_ARGS --font-bold $PLEX_BOLD"
    FONT_ARGS="$FONT_ARGS --font-sans $PLEX_REGULAR --font-fallback $PLEX_REGULAR"
    [ -f "$PLEX_BOLD" ] && FONT_ARGS="$FONT_ARGS --font-sans-bold $PLEX_BOLD"
fi
if [ -f "$MONO_REGULAR" ]; then
    FONT_ARGS="$FONT_ARGS --font-mono $MONO_REGULAR"
    [ -f "$MONO_BOLD" ] && FONT_ARGS="$FONT_ARGS --font-mono-bold $MONO_BOLD"
fi
[ -f "$MATH_FACE" ] && FONT_ARGS="$FONT_ARGS --font-math $MATH_FACE"
METRICS_ARGS=""
[ -f "$METRICS" ] && METRICS_ARGS="--metrics $METRICS"

echo "== gate S: start =="
cargo build -q -p soma-studio
# shellcheck disable=SC2086
target/debug/soma-studio \
    --input "$WORK/main.md" \
    --template "$WORK/template.yaml" \
    $METRICS_ARGS $FONT_ARGS \
    --port "$PORT" > "$WORK/server.log" 2>&1 &
PID=$!
for _ in 1 2 3 4 5 6 7 8 9 10; do
    if curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
        break
    fi
    sleep 0.5
done
if ! curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
    cat "$WORK/server.log" >&2
    echo "soma-studio did not start" >&2
    exit 1
fi

echo "== gate S: S1-S11 (chromium) =="
if command -v chromium >/dev/null 2>&1; then
    timeout 180s chromium --headless --no-sandbox --disable-gpu \
        --hide-scrollbars --user-data-dir="$CHROME_DIR" \
        --virtual-time-budget=60000 --window-size=1920,1080 --dump-dom \
        "http://127.0.0.1:$PORT/?selftest=1&theme=dark" > "$WORK/dom.html" 2>"$WORK/chrome.err" || true
    grep -oE 'S[0-9]+\|(ok|fail)\|[^<]*' "$WORK/dom.html" > "$WORK/results.txt" || true
    cat "$WORK/results.txt"
    for id in S1 S2 S3 S4 S5 S6 S7 S8 S9 S10 S11; do
        if ! grep -q "^$id|ok|" "$WORK/results.txt"; then
            fail "$id missing or failed"
        fi
    done
else
    echo "chromium not found; skipping the browser checks" >&2
fi

echo "== gate S: S4 component CSS has no hex =="
if grep -nE '#[0-9a-fA-F]{3,8}' crates/soma-studio-ui/style/studio.css; then
    fail "studio.css must use tokens.css variables, not hex values"
fi

echo "== gate S: S10 assistant has no engine write path =="
if grep -nE 'studio\.(build|source|template|engine_text|metrics_bytes|hyphenation)\.set|rebuild_now|break_paragraph|soma_core::' \
    crates/soma-studio-ui/src/ai.rs; then
    fail "ai.rs must not write engine inputs or call the breaker"
fi

echo "== gate S: S8 CSP header =="
headers="$(curl -sI "http://127.0.0.1:$PORT/")"
printf '%s\n' "$headers" | grep -qi 'content-security-policy:' || fail "missing CSP header"
printf '%s\n' "$headers" | grep -qi 'x-content-type-options: nosniff' || fail "missing nosniff"

echo "== gate S: S5/S9 native verify, PDF, SSE =="
project_json="$(curl -sf "http://127.0.0.1:$PORT/api/project")"
printf '%s' "$project_json" | grep -q '"customHyphenation":true' \
    || fail "project did not report the discovered hyphen-en.pat"
curl -sf "http://127.0.0.1:$PORT/api/hyphenation" | grep -q 'hy3ph' \
    || fail "/api/hyphenation did not serve the patterns"
summary="$(curl -sf -X POST -H 'content-type: application/json' -d '{}' "http://127.0.0.1:$PORT/api/verify")"
native_pages="$(printf '%s' "$summary" | sed -n 's/.*"pages":\([0-9]*\).*/\1/p')"
native_hash="$(printf '%s' "$summary" | sed -n 's/.*"hash":"\([0-9a-f]*\)".*/\1/p')"
echo "native: $native_pages pages, hash $native_hash"
if ! curl -sf -X POST -H 'content-type: application/json' -d '{}' \
    "http://127.0.0.1:$PORT/api/pdf" -o "$WORK/out.pdf"; then
    fail "PDF export failed"
elif [ -s "$WORK/out.pdf" ]; then
    pdf_pages="$(grep -c '/Type /Page ' "$WORK/out.pdf" || true)"
    echo "pdf: $pdf_pages pages"
    [ "$pdf_pages" = "$native_pages" ] || fail "PDF page count $pdf_pages != native $native_pages"
fi

curl -sN --max-time 30 "http://127.0.0.1:$PORT/api/events" > "$EVENTS" &
CURL_PID=$!
sleep 1
printf '\n\nSSE 增量段落。\n' >> "$WORK/main.md"
sleep 2
grep -q '"type":"built"' "$EVENTS" || fail "SSE did not emit built after a source change"

cp "$WORK/template.yaml" "$WORK/template.bak"
printf 'page: [\n' > "$WORK/template.yaml"
sleep 2
grep -q '"type":"error"' "$EVENTS" || fail "SSE did not emit error after a broken template"
summary2="$(curl -sf -X POST -H 'content-type: application/json' -d '{}' "http://127.0.0.1:$PORT/api/verify")" \
    || fail "server lost its previous build after a failed reload"
printf '%s' "$summary2" | grep -q '"pages"' || fail "old build summary missing after a failed reload"
cp "$WORK/template.bak" "$WORK/template.yaml"
sleep 2
grep -q '"type":"built"' "$EVENTS" || fail "SSE did not emit built after restoring the template"
kill "$CURL_PID" 2>/dev/null || true
CURL_PID=""

echo "== gate S: S9 without fonts =="
if command -v chromium >/dev/null 2>&1; then
    # shellcheck disable=SC2086
    target/debug/soma-studio \
        --input "$WORK/main.md" \
        --template "$WORK/template.yaml" \
        $METRICS_ARGS --no-auto-font \
        --no-watch --port "$FONTLESS_PORT" > "$WORK/fontless.log" 2>&1 &
    FONTLESS_PID=$!
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        if curl -sf "http://127.0.0.1:$FONTLESS_PORT/api/health" >/dev/null 2>&1; then
            break
        fi
        sleep 0.5
    done
    rm -rf "$CHROME_DIR-fontless"
    timeout 120s chromium --headless --no-sandbox --disable-gpu \
        --hide-scrollbars --user-data-dir="$CHROME_DIR-fontless" \
        --virtual-time-budget=30000 --window-size=1920,1080 --dump-dom \
        "http://127.0.0.1:$FONTLESS_PORT/?selftest=1&only=s9font&theme=dark" \
        > "$WORK/dom-fontless.html" 2>/dev/null || true
    fontless="$(grep -oE 'S9\|(ok|fail)\|[^<]*' "$WORK/dom-fontless.html" | head -1 || true)"
    echo "$fontless"
    printf '%s' "$fontless" | grep -q '^S9|ok|' || fail "missing-font degradation did not pass"
    printf '%s' "$fontless" | grep -q '不一致' || fail "missing-font probe did not report 不一致"
    kill "$FONTLESS_PID" 2>/dev/null || true
    FONTLESS_PID=""
fi

if [ "$FAILED" = "1" ]; then
    echo "studio gates failed" >&2
    exit 1
fi
echo "all studio gates passed"
