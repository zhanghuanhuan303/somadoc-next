#!/bin/sh
# Gate W: build the browser slice and verify it without a bundler.
#
#   W1 zero imports          the module instantiates with `{}`
#   W2 size                  gzip budget
#   W3 parity with native    the committed fixture, in Chromium
#   W4 timing                a 2k-atom paragraph stays inside the frame budget
#   W5 memory                repeated breaks do not grow the heap
#   W6 fallback policy       a narrow measure can always fall back to greedy
#
# Requires `wasm32-unknown-unknown`; uses node and/or chromium when present.
set -eu

cd "$(dirname "$0")/.."

WASM=target/wasm32-unknown-unknown/web/soma_web.wasm
OUT=web/soma-kp.wasm
BUDGET_BYTES=40000

echo "== gate W: build =="
cargo build -p soma-web --profile web --target wasm32-unknown-unknown
cp "$WASM" "$OUT"
raw=$(wc -c < "$OUT")
gz=$(gzip -9 -c "$OUT" | wc -c)
echo "built $OUT (${raw} bytes raw, ${gz} bytes gzip)"

echo "== gate W: fixture is current =="
cargo run -q -p soma-web --example fixture > /tmp/soma-parity.txt
if cmp -s /tmp/soma-parity.txt web/parity.txt; then
    echo "fixture unchanged"
else
    echo "parity.txt is stale. Regenerate with" >&2
    echo "  cargo run -p soma-web --example fixture > web/parity.txt" >&2
    echo "and explain the break change in the commit (breaks must not drift silently)." >&2
    exit 1
fi

echo "== gate W: ABI parity (native) =="
cargo test -q -p soma-web --test abi

if command -v node >/dev/null 2>&1; then
    echo "== gate W: W1/W2/W4/W5/W6 (node) =="
    node web/check.mjs "$OUT" "$gz" "$BUDGET_BYTES"
else
    echo "node not found; skipping the headless checks" >&2
fi

if command -v chromium >/dev/null 2>&1; then
    echo "== gate W: W1-W6 (chromium) =="
    chromium --headless --no-sandbox --disable-gpu \
        --allow-file-access-from-files \
        --virtual-time-budget=20000 --dump-dom \
        "file://$PWD/web/harness.html" 2>/dev/null \
        | grep -oE 'W[1-6]-[a-z]+\|(ok|FAIL)\|[^<]*' > /tmp/soma-harness.txt || true
    cat /tmp/soma-harness.txt
    if grep -q '|FAIL|' /tmp/soma-harness.txt; then
        echo "browser self-check failed" >&2
        exit 1
    fi
    if [ "$(grep -c '|ok|' /tmp/soma-harness.txt)" -lt 5 ]; then
        echo "browser self-check did not run; is fetch blocked?" >&2
        exit 1
    fi
else
    echo "chromium not found; skipping the browser self-check" >&2
fi

echo "all web gates passed"
