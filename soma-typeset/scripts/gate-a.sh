#!/usr/bin/env bash
# Gate A (architecture). Layer boundaries and layer-owned contract constants.
#
# K-P gates live in gates.sh; this file is specific to the A-phase layers and
# grows as soma-template / soma-layout / soma-render-* land.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== gate A: soma-doc stays dependency-free =="
deps="$(cargo tree -p soma-doc --edges normal --prefix none)"
if [ "$(printf '%s\n' "$deps" | wc -l)" -ne 1 ]; then
    printf '%s\n' "$deps" >&2
    echo "soma-doc must not depend on any crate (DocIR has no physical units)" >&2
    exit 1
fi

echo "== gate A: soma-doc is no_std with alloc only =="
grep -q '#!\[cfg_attr(not(test), no_std)\]' crates/soma-doc/src/lib.rs
if grep -q 'extern crate std' crates/soma-doc/src/lib.rs; then
    echo "soma-doc must not link std" >&2
    exit 1
fi

echo "== gate A: soma-template depends only on DocIR and the kernel =="
for dep in $(cargo tree -p soma-template --edges normal --prefix none | awk '{print $1}' | sort -u); do
    case " $dep " in
        " soma-template "|" soma-core "|" soma-doc ") ;;
        *)
            echo "soma-template must not depend on $dep" >&2
            exit 1
            ;;
    esac
done

if grep -REn '\b(f32|f64)\b' crates/soma-template/src --include='*.rs' \
    | grep -vE ':[0-9]+:[[:space:]]*//'; then
    echo "soma-template must stay integer-only" >&2
    exit 1
fi

if grep -q 'extern crate std' crates/soma-template/src/lib.rs; then
    echo "soma-template must stay no_std" >&2
    exit 1
fi

echo "== gate A: layer schemas and dependency direction =="
check_deps() {
    pkg="$1"
    allowed="$2"
    for dep in $(cargo tree -p "$pkg" --depth 1 --edges normal --prefix none | awk '{print $1}' | sort -u); do
        case " $allowed " in
            *" $dep "*) ;;
            *)
                echo "$pkg must not depend on $dep" >&2
                exit 1
                ;;
        esac
    done
}

check_deps soma-layout "soma-layout soma-core soma-doc soma-metrics soma-rules soma-template"
check_deps soma-md "soma-md soma-doc"
check_deps soma-engine "soma-engine soma-core"
check_deps soma-metrics "soma-metrics"
check_deps soma-render-html "soma-render-html soma-core soma-layout soma-template"
check_deps soma-render-svg "soma-render-svg soma-core soma-layout soma-template"

for crate in soma-engine soma-layout soma-md soma-metrics soma-render-html soma-render-svg; do
    if grep -q 'extern crate std' "crates/$crate/src/lib.rs"; then
        echo "$crate must stay no_std" >&2
        exit 1
    fi
done

if grep -REn '\b(f32|f64)\b' crates/soma-layout/src crates/soma-md/src crates/soma-metrics/src crates/soma-render-html/src crates/soma-render-svg/src crates/soma-engine/src \
    --include='*.rs' | grep -vE ':[0-9]+:[[:space:]]*//'; then
    echo "layout, markdown, metrics, render, and engine must stay integer-only" >&2
    exit 1
fi

echo "== gate A: soma-core stays below soma-doc =="
if cargo tree -p soma-core --edges normal | grep -q 'soma-doc'; then
    echo "soma-core must not depend on soma-doc" >&2
    exit 1
fi

echo "== gate A: soma-core no longer exports the legacy semantic seed =="
if grep -Eq 'pub use (semantic|text)::' crates/soma-core/src/lib.rs; then
    echo "legacy semantic/text exports must be gone (ADR-0008)" >&2
    exit 1
fi
if [ -e crates/soma-core/src/semantic.rs ] || [ -e crates/soma-core/src/text.rs ]; then
    echo "legacy semantic/text modules must be deleted (ADR-0008)" >&2
    exit 1
fi

echo "== gate A: no floating point in DocIR =="
if grep -REn '\b(f32|f64)\b' crates/soma-doc/src --include='*.rs'; then
    echo "soma-doc must stay integer-only" >&2
    exit 1
fi

echo "== gate A: layer schema versions are declared =="
grep -q 'pub const SCHEMA: ' crates/soma-doc/src/doc.rs
grep -q '"soma.doc/1"' crates/soma-doc/src/doc.rs
grep -q 'pub const DOC_IR_VERSION: u16 = 1' crates/soma-doc/src/doc.rs
grep -q 'soma.layout/6' crates/soma-core/src/result.rs
grep -q '"soma.template/5"' crates/soma-template/src/template.rs
grep -q '"soma.styles/2"' crates/soma-template/src/canonical.rs
grep -q '"soma.pages/4"' crates/soma-layout/src/canonical.rs
grep -q '"soma.engine/1"' crates/soma-engine/src/lib.rs

echo "== gate A: DocIR and templates carry no physical style in content =="
if grep -REn '"?(width|font_size|fontSize|margin|color|colour|line_height|lineHeight)"?\s*:' \
    crates/soma-doc/src --include='*.rs'; then
    echo "DocIR must not carry physical style fields (ADR-0009)" >&2
    exit 1
fi

if grep -REn '"(tolerance|badness_scale|max_kp_atoms|preset)"\s*:' \
    crates/soma-template/src --include='*.rs'; then
    echo "templates must not accept engine knobs (ADR-0009 §5)" >&2
    exit 1
fi

echo "gate A passed"
