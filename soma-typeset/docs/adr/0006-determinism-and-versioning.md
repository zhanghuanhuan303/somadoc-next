# ADR-0006: Determinism, units, and versioning

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §7.4, §12

## Context

The existing core is integer-only (`Width(i32)` in 1/4096 em), emits canonical
JSON with fixed field order, and has a native/`wasm32-wasip1` hash gate. The new
document/template/page layers must preserve that property; templates and page
geometry are the main new risks because authors write decimals such as `10.5pt`
and `18mm`.

## Decision

- All layout geometry is integer fixed-point. `soma-core` and `soma-layout`
  contain no `f32`/`f64`.
- Template decimals are parsed as decimal rationals at the edge and converted
  with a fixed scale and round-half-up. The rounding rule is part of
  `templateVersion`.
- Each layer has a canonical JSON form:
  - DocIR: `soma.doc/1`;
  - Template: `soma.template/1` + `soma.styles/1`;
  - Layout Box: `soma.boxes/1`;
  - Page result: `soma.pages/1`;
  - Inline line result: current `soma.layout/6`.
- Field order and enum strings are frozen per schema. Unknown-field behavior is
  documented; cross-schema byte comparison is invalid.
- The reproducibility tuple is:
  `documentHash + templateHash + styleHash + rulesVersion + metricsVersion +
  enginePreset/paramHash + layoutModelVersion + paginationVersion`.
- `totalCost` is only comparable within the same fallback stage and the same
  tuple.
- Native and `wasm32-wasip1` must produce identical canonical bytes; the
  existing `scripts/wasm-hash.sh` gate is extended to the full pipeline.
- No wall-clock decisions, no hash-map iteration order, no locale-dependent
  formatting in canonical output.

## Consequences

- A unit-conversion module and decimal parser are required at the template edge.
- More schema versions and golden tests.
- Templates cannot use arbitrary CSS units/functions.
- Cross-target tests remain cheap because the core never introduces floats.

## Alternatives rejected

- Floats in template/layout: breaks cross-target byte identity and introduces
  platform-dependent rounding.
- Reusing CSS/SVG unit semantics: too broad and underspecified for print.
- A single global schema version for all layers: prevents independent evolution
  and makes diagnostics ambiguous.

## Alignment with current code

- `crates/soma-core/src/units.rs`: integer units.
- `crates/soma-core/src/params.rs`: version/hash pattern.
- `crates/soma-core/src/result.rs`: canonical writer pattern.
- `scripts/wasm-hash.sh`: cross-target proof.
