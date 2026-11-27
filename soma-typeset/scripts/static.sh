#!/bin/sh
# Gate static: the public Studio is a pure static bundle.
#
# It serves `dist/` with a plain file server (no Axum), runs the browser
# self-test with `?local=1`, asserts S1/S3-S10/S12-S14, and proves that a local
# session never requests `/api`. `/api/health` must be a 404 from the static
# server.
#
#   bash scripts/static.sh
#   SOMA_STATIC_SKIP_TRUNK=1 bash scripts/static.sh   # reuse dist/
#   SOMA_STATIC_PORT=7094 bash scripts/static.sh
#
# The fonts are not in the repository. Set SOMA_FONT_DIR (and pyftsubset) when
# `dist/fonts/v2` is empty; the deploy build already copies them.
set -eu

cd "$(dirname "$0")/.."

PORT="${SOMA_STATIC_PORT:-7093}"
DIST="crates/soma-studio-ui/dist"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/soma-static-gate.XXXXXX")"
PID=""
FAILED=0

cleanup() {
    [ -n "$PID" ] && kill "$PID" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

fail() {
    echo "static-gate: $1" >&2
    FAILED=1
}

echo "== static gate: build =="
if [ "${SOMA_STATIC_SKIP_TRUNK:-0}" != "1" ] && command -v trunk >/dev/null 2>&1; then
    (cd crates/soma-studio-ui && trunk build --release >/dev/null)
else
    echo "using existing crates/soma-studio-ui/dist"
fi
if [ ! -f "$DIST/index.html" ]; then
    echo "crates/soma-studio-ui/dist is missing; run trunk build" >&2
    exit 1
fi

if [ ! -f "$DIST/fonts/v2/IBMPlexSansSC_Regular.ttf" ]; then
    if [ -n "${SOMA_FONT_DIR:-}" ] && command -v pyftsubset >/dev/null 2>&1; then
        SOMA_FONT_DIR="$SOMA_FONT_DIR" bash scripts/local-assets.sh >/dev/null
    else
        echo "static-gate: dist/fonts/v2 is missing; set SOMA_FONT_DIR and install pyftsubset" >&2
        exit 1
    fi
fi
cp examples/book/welcome.md "$DIST/welcome.md"

echo "== static gate: serve (no Axum) =="
python3 - "$PORT" "$DIST" >"$WORK/http.log" 2>&1 <<'PY' &
import http.server
import socketserver
import sys

port = int(sys.argv[1])
root = sys.argv[2]


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=root, **kwargs)

    def end_headers(self):
        self.send_header(
            "Content-Security-Policy",
            "default-src 'self'; img-src 'self' data: blob:; font-src 'self' data:; "
            "style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline' "
            "'wasm-unsafe-eval'; connect-src 'self'; object-src 'none'; "
            "base-uri 'none'; frame-ancestors 'none'",
        )
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        super().end_headers()

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (self.address_string(), fmt % args))


Handler.extensions_map.update(
    {
        ".wasm": "application/wasm",
        ".ttf": "font/ttf",
        ".otf": "font/otf",
        ".md": "text/markdown; charset=utf-8",
    }
)

with socketserver.TCPServer(("127.0.0.1", port), Handler) as httpd:
    httpd.serve_forever()
PY
PID=$!

for _ in 1 2 3 4 5 6 7 8 9 10; do
    if curl -sf "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then
        break
    fi
    sleep 0.5
done
curl -sf "http://127.0.0.1:$PORT/" >/dev/null || {
    cat "$WORK/http.log" >&2
    echo "static server did not start" >&2
    exit 1
}

echo "== static gate: assets =="
for path in index.html welcome.md fonts/v2/IBMPlexSansSC_Regular.ttf; do
    curl -sf "http://127.0.0.1:$PORT/$path" >/dev/null || fail "missing $path"
done
curl -sI "http://127.0.0.1:$PORT/" | grep -qi 'content-security-policy:' \
    || fail "missing CSP header"
curl -sI "http://127.0.0.1:$PORT/" | grep -qi 'x-content-type-options: nosniff' \
    || fail "missing nosniff"

echo "== static gate: S1/S3-S10/S12-S14 (chromium, no Axum) =="
if command -v chromium >/dev/null 2>&1; then
    timeout 240s chromium --headless --no-sandbox --disable-gpu \
        --hide-scrollbars --user-data-dir="$WORK/chrome" \
        --virtual-time-budget=90000 --window-size=1920,1080 --dump-dom \
        "http://127.0.0.1:$PORT/?selftest=1&local=1&theme=dark" \
        > "$WORK/dom.html" 2>"$WORK/chrome.err" || true
    grep -oE 'S[0-9]+\|(ok|fail)\|[^<]*' "$WORK/dom.html" > "$WORK/results.txt" || true
    cat "$WORK/results.txt"
    for id in S1 S3 S4 S5 S6 S7 S8 S9 S10 S12 S13 S14; do
        if ! grep -q "^$id|ok|" "$WORK/results.txt"; then
            fail "$id missing or failed"
        fi
    done
    if grep -q '/api' "$WORK/http.log"; then
        grep '/api' "$WORK/http.log" >&2
        fail "a local session requested /api"
    fi
else
    fail "chromium is required for the static gate"
fi

echo "== static gate: /api is absent =="
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/api/health")"
[ "$code" = "404" ] || fail "/api/health returned $code, expected 404"

if [ "$FAILED" = "1" ]; then
    echo "static gate failed" >&2
    exit 1
fi
echo "all static gates passed"
