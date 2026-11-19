# K3 report

Status: **K3 green**. Style metrics and line boxes are part of the result; tall
inline atoms and baseline shifts no longer require special breaker branches.

## Delivered

- `GlyphMetrics` with `advance`, `ascent`, `descent`, `leading`,
  `baseline_shift`, and visual `overhang_left/right`.
- `Metrics::glyph_metrics` default implementation; hosts may override it.
- `Line.ascent/descent/leading/height`; canonical schema is now
  `soma.layout/3`.
- Line box:
  `ascent = max(ascent + baseline_shift)`,
  `descent = max(descent - baseline_shift) >= 0`,
  `leading = max(leading)`,
  `height = max(content_height, style.line_height)`.
- Inline large atoms (for example 2em) change advance and line box only; the
  break rules are unchanged.
- `features.height_cost` + `height_scale` adds
  `height_scale * max(0, height - height_threshold) / body_height`.
  Default remains disabled.
- Trace includes line-box metrics.
- Public test proves a word-internal style boundary marked `Forbidden` is not
  broken; the rule layer is responsible for emitting that boundary.

## Tests

`crates/soma-core/tests/k3_styles.rs`:

1. `glyph_metrics_default_uses_advance_and_line_metrics`
2. `large_inline_atom_increases_the_line_box`
3. `baseline_shift_changes_ascent_and_descent`
4. `word_internal_style_split_stays_unbreakable`
5. `canonical_result_contains_line_box_metrics`
6. `height_cost_is_enabled_by_flag`

## Verification

Benchmark after K3 (native release, synthetic metrics, pure CJK):

```text
atoms=2000 lines=100 elapsed_ms=2.28
atoms=4000 lines=200 elapsed_ms=7.90
```

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 60 passed
  wasm32 check: pass
  soma-core dependency boundary: pass
```

## Deferred

- Rule-layer style-run input (the core contract is tested; style parsing belongs
  to the Markdown/rule front end).
- Edge trim and box-side trimming (K4).
- Forced breaks/segments (K5).
- `max_kp_atoms` result flags.
