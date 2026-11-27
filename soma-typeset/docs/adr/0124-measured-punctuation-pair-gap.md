# ADR-0124: Adjacent punctuation closes to a measured ink gap

- Status: Accepted; the dynamic shrink reserve added by ADR-0125 is layered on top of this baseline
- Date: 2026-11-28
- Related: ADR-0036 (line-end trim), ADR-0037 (punctuation ink), ADR-0082
  (line-start trim), ADR-0105 (ragged opening trim), ADR-0123 (measured list
  marker hang)
- Touches: `soma-core` (`Boundary`, breaker natural width/bounds/boxes,
  `compression_cost`), `soma-rules` (`pair_gap`, trim caps), `soma-layout`
  (`apply_pair_gap`), tests, `docs/kp-model.md`, `docs/params.md`

## Context

The rule table already gives every punctuation atom a left/right side-bearing
budget (`compress_left` / `compress_right`). Until now only the **line edges**
spent that budget: a closing mark at the end of a line, an opening mark at the
start. Two marks set against each other in the middle of a line kept their full
advances, even when their glyphs left a visibly larger hole than the same pair
at a line edge (`。”（`, `），`, `“（`). The old `PAIR_TRIM` constant tried to
address this by adding 0.25 em to the second mark's `compress_left`, but it only
took effect when that mark happened to land at a line start, so it did not
tighten the pair in normal text.

The gap is a property of the **face**, not only of the class. ADR-0037 already
records `ink_left` / `ink_right` per punctuation glyph; the same data can answer
“how much empty space is actually between these two outlines?”.

## Decision

1. **Add a fixed pair gap to the boundary.**
   `Boundary` gains `pair_squeeze: Width`. It is a deterministic width
   reduction, not a glue stretch: a line that keeps the boundary inside is
   narrower by that amount, and the boxes after the boundary move left by the
   same amount. A line that breaks at the boundary excludes it, exactly like
   glue; each edge is then handled by trim.

2. **The target is an ink gap, not a reduction.**
   For adjacent punctuation atoms `left` and `right`:

   ```text
   gap_ink = max(0, ink_right(left) + ink_left(right))
   target  = pair_target(class_left, class_right, full_width, max(font_size))
   cap     = pair_cap(class_left, class_right, full_width, max(font_size))
   squeeze = clamp(gap_ink - target, 0, cap)
   ```

   The starting targets are 0.25 em for a full-width pair and 0.125 em for a
   half-width pair; `Open → Close` (empty quotes/brackets) uses half of those.
   Caps are 0.5 em / 0.125 em. They are `ParamOnly` until the corpus and font
   measurements in P0 settle them.

3. **The pair table is class-based and deliberately small.**
   Supported pairs: `Close → Open`, `Close → Close`, `Close → Middle`,
   `Middle → Open`, `Middle → Close`, `Open → Open`, `Open → Close`.
   `Dash` (ellipsis, em dash, middle dot) is excluded: it carries its own space.
   `Middle → Middle` is excluded for now; it is an unusual sequence and would
   make the table noisy.

4. **Eligibility is semantic, not script-specific in the breaker.**
   `soma-layout::apply_pair_gap` runs after `scale_trim`, uses the atom classes
   and the metrics table, and writes the result. It skips spaces, forced
   breaks, hyphens, `Dash`, inline objects, ruby bases, and math/code atoms
   (which carry no punctuation class). Pair tightening works across bold/italic
   boundaries, because that is still body punctuation.

5. **The breaker stays O(n²) and stateless.**
   `Shaped` keeps a prefix sum of `pair_squeeze`; `natural(start, end)` subtracts
   the internal pairs, and `build_boxes` subtracts each pair from the gap. The
   candidate-window bound subtracts `(k - 1) * max_pair_squeeze` from the
   overfull margin so a line that only fits after pair tightening is never
   pruned. The DP has no new state.

6. **Edge trim and pair gap are separate costs.**
   Pair gap is a rule, so it does not add a cost term; the DP sees the true
   width. Edge trim remains priced by `compress_amount` and `compress_count`,
   and `compress_count` now counts the two edges independently
   (`[trim_left > 0] + [trim_right > 0]`) instead of charging once per line.
   Optical trim on an underfull justified line is still free; compensation on an
   originally overfull line is still charged.

7. **Versions.** `model_version` 6 (natural width, bounds, box coordinates and
   the cost count change) and `rules_version` 3 (pair targets/caps, removal of
   `PAIR_TRIM`). The canonical schema id is unchanged: `pair_squeeze` is
   encoded in `naturalWidth` and box `x` positions; trace-only pair totals are
   outside the canonical result. `engine_hash` now includes the model and rule
   versions, so a version bump cannot reuse a stale hash.

## Consequences

- `。”（`, `），`, `“（` and similar pairs close to a consistent optical gap in
  every face that has outline ink. A face without ink (CFF, synthetic test
  metrics) falls back to the class side bearings.
- A pair split across a line break keeps the old edge behavior: pair tightening
  is excluded at the break, and each mark uses its own trim budget.
- Line breaks and page counts move; every canonical result and `engineHash`
  changes because `modelVersion`/`rulesVersion` are in the hash tuple.
- `PAIR_TRIM` is gone. A line starting with a closing/middle mark no longer gets
  an extra 0.25 em left bonus from the removed pair constant; `scale_trim` still
  provides the glyph's own ink bearing when the metrics table has it.
- `compress_count` now charges per edge. Existing cost defaults stay
  (`compress_count = 5`), so a line trimmed on both sides pays twice; corpus
  calibration may lower it in the next preset.
- List markers are unaffected: `•` carries no punctuation class, and the marker
  gap is pinned by ADR-0123.
- The trace gains `pair_squeeze` and `pair_squeeze_count`; the Studio inspect
  pane can show them without a canonical schema change.

## Alternatives considered

- **Make every punctuation boundary a shrinkable glue.** Rejected: justification
  would squeeze commas in the middle of a line uniformly, which is the behavior
  ADR-0036 deliberately avoided. Pair gap is a fixed typographic target, not an
  emergency shrink.
- **Keep `PAIR_TRIM` and only apply it at line starts.** Rejected: it solves the
  rare fallback case and not the reported normal-text hole; it also
  double-counts once pair gap exists.
- **A full codepoint-pair table.** Rejected for the first version: a 3×3 class
  table plus measured ink covers the visible cases, and exceptions can be added
  with a `ruleId` when a real document shows one.
- **Conditional mid-line squeeze only when a line is overfull.** Deferred: it
  would make spacing depend on the chosen break and needs a line-level selection
  policy. The fixed target is predictable, cheaper, and sufficient for v2.
- **Hanging the second mark instead of tightening it.** Rejected as the default:
  ADR-0036 explains why hanging moves the hole rather than closing it for faces
  whose closing marks sit at the left of their advance.
