# K5 report

Status: **K5 green**. Forced breaks are modelled as hard segment boundaries,
not as negative penalties. Canonical schema is now `soma.layout/5`.

## Delivered

- `Breakability::Forced` is a hard break. `split_segments` scans the boundary
  list once and cuts the paragraph into non-empty `Segment { start, end,
  is_paragraph_end }` values. The internal segment list is not part of the
  public IR.
- Each segment runs its own DP / greedy pass. Internal breaks must be
  `Allowed`; the segment end is always available.
- Segment-end semantics:
  - `LineEnd::ParagraphEnd` (last segment): trim, then either `short_score` or
    `overfull + emergency`.
  - `LineEnd::ForcedEnd` (non-final segment): trim, then either no penalty or
    `overfull + emergency`. It is **not stretched** and does **not** pay
    `short_score`.
  - Both still pay the normal line cost, per `kp-model.md` §2.6.3.
- `Line.forced_end` records a forced segment end. Canonical field order adds
  `forcedEnd` after `emergency`.
- The next segment starts at a global atom index greater than 0, so the first
  line after a forced break gets no `first_indent`; the target-width formula is
  unchanged and remains `start == 0` based.
- `features.forced_breaks == false` + an IR containing `Forced` returns
  `BreakError::ForcedBreaksDisabled`. It is never silently treated as
  `Allowed`.
- Greedy-F and Greedy-C also break at segments; they cannot cross a forced
  boundary.
- The rules layer exposes `paragraph_from_lines`: every boundary between two
  input lines is emitted as `Forced`, giving the Markdown hard-break/verse case
  a public entry point without a Markdown parser. Empty input lines are
  rejected.
- The internal brute-force reference enumerates each segment independently, so
  optimality with forced breaks is still checked against exhaustive
  enumeration (not against the DP itself).
- Global `max_lines` is checked after all segments merge (optimal) or while
  emitting lines (greedy).

## Tests

`crates/soma-core/tests/k5_forced.rs` (7):

1. `forced_break_splits_segments`
2. `forced_segment_no_indent`
3. `forced_end_does_not_stretch_or_short_score`
4. `consecutive_forced_breaks_create_single_atom_lines`
5. `greedy_respects_forced_breaks`
6. `canonical_result_contains_forced_end`
7. `forced_breaks_disabled_is_rejected`

Internal unit test:

- `brute_force_matches_dp_with_forced_breaks` — exhaustive enumeration over
  both segments agrees with the DP and the first line is flagged
  `forced_end`.

Rules-layer tests:

- `paragraph_from_lines` unit test: an empty hard-break line is rejected.
- `markdown_hard_break_is_forced` in `vertical_slice.rs`: two source lines
  become two layout lines, the first marked `forced_end`, and the second line
  has no first-line indent.

## Verification

Benchmark after K5 (native release, synthetic metrics, pure CJK):

```text
atoms=2000 lines=100 elapsed_ms=2.46
atoms=4000 lines=200 elapsed_ms=8.66
```

Forced segments add one linear scan; the no-forced path is still the same DP.

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 83 passed
  wasm32 check: pass
  soma-core dependency boundary: pass
```

## Decisions recorded

- **Global indices, segment-local DP tables.** The DP uses local arrays of
  length `segment.end - segment.start + 1`, but calls `eval_line` with global
  atom indices. This keeps prefix sums global and makes `start == 0` a safe
  test for the paragraph's first line. The spec's `is_paragraph_start &&
  local_i == 0` is therefore satisfied without a second offset.
- **Forced end beats short-score.** A hard break is an author decision; the
  line before it is never penalized for being short. The paragraph's final
  segment still pays `short_score`.
- **No implicit indent reset.** A forced break does not start a new paragraph;
  only global atom 0 can carry `first_indent`.
- **Mandatory even when wasteful.** A paragraph wide enough for one perfect
  line still splits at a forced boundary. The golden asserts two line costs
  where the ignored single-line path would cost one.
- **Schema bump to `/5`.** `forcedEnd` changes canonical bytes; cross-schema
  byte comparisons are invalid.

## Deferred

- Markdown syntax parsing (two trailing spaces, backslash, `<br>`); the text
  compiler still collapses whitespace. `paragraph_from_lines` is the IR seam
  the Markdown front end will call.
- `LayoutResult.breaker_used` / `length_limited` / `fallback_stage` flags.
- `max_kp_atoms` + Greedy-C fallback for very long paragraphs.
- ~~`RuleStatus` inventory (`Verified` / `Tbd` / `ParamOnly`) and per-rule
  source table.~~ Landed in K6/K8 as `soma_rules::RULES` (8 `Verified`,
  0 `Tbd`, 6 `ParamOnly`).
- Page/chapter windows and active-node pruning.
