# K2 report

Status: **K2 green**. The cost model now includes CjkLatin boundary deviation,
the result carries per-boundary evidence, and the trace API explains each line.

## Delivered

- `BoundaryRole::{Plain, CjkLatin}` on `Boundary`; `soma-rules` marks CJK/Latin
  no-space boundaries as `CjkLatin`.
- `Line.boundary_deviation`: O(1) approximate mean absolute deviation.
- `Line.cjk_latin_adjustments`: actual boundary index + adjustment for every
  CjkLatin boundary; eval/renderer can verify the proportional assumption.
- CjkLatin deviation cost:
  `boundary_deviation * D / ideal_gap`, with `ideal_gap` derived from the
  boundary glue (fallback 1em if zero).
- `break_paragraph_traced` + `LineTrace` + `KpTrace`:
  line cost breakdown, ratio, badness, short-last, boundary deviation,
  evaluated candidate count, fallback flag. Trace is not canonical.
- Canonical schema bumped to `soma.layout/2` with `boundaryDeviation` and
  `cjkLatinAdjustments`. K3 later moved the current schema to `/3` for line
  boxes; `/2` remains the K2 byte contract.

## Tests

`crates/soma-core/tests/k2_boundary.rs`:

1. `cjk_latin_adjustments_match_the_line_adjustment`
2. `boundary_deviation_uses_the_proportional_assumption`
3. `canonical_result_contains_boundary_evidence`
4. `trace_explains_each_line_without_changing_layout`
5. `trace_marks_fallback_lines`

## Verification

Benchmark after K2 (native release, synthetic metrics, pure CJK):

```text
atoms=2000 lines=100 elapsed_ms=1.87
atoms=4000 lines=200 elapsed_ms=7.40
```

`default-v1.max_kp_atoms` remains `None`.

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 54 passed
  wasm32 check: pass
  soma-core dependency boundary: pass
```

## Deferred

- `max_kp_atoms` result flags and `length_limited` reporting (K1 decision was
  `None`; benchmark is in `docs/k1-report.md`).
- Edge trim, renderer-side box trimming (K4).
- Style/line-height output (K3).
- Forced breaks/segments (K5).
