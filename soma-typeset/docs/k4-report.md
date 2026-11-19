# K4 report

Status: **K4 green**. Edge trimming is part of the cost model and the emitted
layout; the canonical schema is now `soma.layout/4`.

## Delivered

- `Atom.compress_left` / `Atom.compress_right` are consumed only when
  `natural_width > target_width` and `features.edge_trim == true`.
- Deterministic order: trim the end first, then the start:

  ```text
  c_end   = min(E, compress_right(last))
  c_start = min(E - c_end, compress_left(first))
  effective = natural - c_end - c_start
  ```

  `E <= 0` never trims. Trim never creates an underfull line because
  `c <= E`.
- `Line.trim_left` / `Line.trim_right` record the amounts; `Line.natural_width`
  stays untrimmed. `Line.ratio_milli` is the post-trim shrink/stretch ratio
  and is `0` on the paragraph last line and on forced segment ends.
- Compression cost:
  `compress_amount * trim / EM + compress_count` when `trim > 0`. The count
  term is one event per line, not per side.
- Rendering coordinate convention:
  first box `x = indent - trim_left`; last box
  `advance = width - trim_right`; internal boxes and glue adjustments are
  unchanged. For a single-atom line both apply to the same box.
- Last-line rule: trim first, then decide. If `effective > target` after trim,
  the normal phase rejects the line and the fallback marks it
  `overfull + emergency`. Last lines do not stretch.
- `Line.boxes` is built from the trimmed widths; the guarded invariant
  `last.x + last.advance == indent + target_width` holds for non-final lines
  and for final lines that trim exactly to the target.
- `TrimPriority::EndFirst` is the only implemented order. While
  `features.edge_trim` is on, `StartFirst` and `Balanced` are rejected by
  `ParamSet::validate` with `break_params.trim_priority` instead of silently
  behaving like `EndFirst`; with trim off the field is ignored.
- Rule layer: opening punctuation gets `open_trim_left = 250‰`, closing
  punctuation `close_trim_right = 500‰`, middle punctuation
  `middle_trim_left/right = 500‰` (base em). The ratios are `ParamOnly` and
  uncalibrated; `pair_trim_table` and per-size scaling are deferred.
- Canonical field order now includes
  `naturalWidth, trimLeft, trimRight, ratioMilli` before `ascent`.
- The breaker still never reads `PunctClass`: a public test proves two atoms
  with identical geometry but different classes produce byte-identical
  layouts.

## Tests

`crates/soma-core/tests/k4_trim.rs` (10):

1. `edge_trim_matches_target`
2. `trim_prefers_end_before_start`
3. `single_atom_double_trim`
4. `double_trim_still_overfull_fallback`
5. `trim_not_applied_when_underfull`
6. `trim_cost_enters_line_cost`
7. `breaker_ignores_punctuation_class`
8. `edge_trim_flag_disables_trim`
9. `canonical_result_contains_trim_fields`
10. `unsupported_trim_priority_is_rejected`

`crates/soma-rules/src/lib.rs` adds
`punctuation_trim_caps_follow_the_rule_table`.

`crates/soma-rules/tests/vertical_slice.rs` adds:

- `trailing_comma_is_trimmed_at_line_end`
- `leading_open_bracket_is_trimmed_at_line_start`

## Verification

Benchmark after K4 (native release, synthetic metrics, pure CJK):

```text
atoms=2000 lines=100 elapsed_ms=2.07
atoms=4000 lines=200 elapsed_ms=8.76
```

The trim branch is one extra comparison on the common underfull path; the
numbers are within run-to-run noise of K3 (2.28 / 7.90).

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 73 passed
  wasm32 check: pass
  soma-core dependency boundary: pass
```

## Decisions recorded

- **Schema bump to `/4`.** `trimLeft` / `trimRight` / `ratioMilli` change the
  canonical bytes, so old and new results must not be compared byte-for-byte.
- **`ratio_milli` is post-trim.** It reports the ratio actually used for
  badness. A trimmed last line stays `0` because it never justifies.
- **One compression event per line.** Both sides share one `compress_count`;
  `compress_amount` scales with the total trimmed width. This keeps the cost a
  function of `(i, j)` only.
- **Unsupported trim priorities are loud.** Validating them is cheaper than
  carrying three untested orders through K5/K6.

## Deferred

- `pair_trim_table` (stacked punctuation) and per-style `font_scale` trim.
- `RuleStatus` inventory and `ruleId × source × status × golden` table (K0
  deliverable, still open).
- Hanging punctuation as negative trim; overhang remains purely visual.
- Forced breaks/segments (K5).
- `max_kp_atoms` result flags (`breaker_used`, `length_limited`).
