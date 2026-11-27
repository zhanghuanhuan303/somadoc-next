# ADR-0125: Punctuation shrink is a dynamic glue reserve

- Status: Accepted
- Date: 2026-11-28
- Related: ADR-0124 (measured pair gap), ADR-0036 (line-end trim), ADR-0041
  (overfull is only reachable through fallback), ADR-0112 (short-line term)
- Touches: `soma-core` (boundary glue shrink, line evaluation), `soma-rules`
  (`punct_shrink_budget`), `soma-layout` (`apply_punctuation_spacing`), tests,
  `docs/params.md`, `docs/kp-model.md`

## Context

ADR-0124 gave adjacent punctuation a measured **baseline** gap. That fixed the
normal-text hole, but a line that is still too wide had only two ways out:

1. trim the side bearing of its first/last mark, or
2. shrink the ordinary glue (spaces, CJK gaps), or
3. run out of shrink and become overfull/emergency.

The punctuation atoms still carried empty side bearings that were never
available to the negative justification path. A Chinese line with several
commas or closing marks can often recover a quarter of an em or more by closing
those bearing gaps slightly, which is much less visible than an overfull line or
a forced emergency break.

The earlier model deliberately kept punctuation out of glue to avoid squeezing
every comma in the middle of a line. The distinction that matters is not
“punctuation may never shrink” but **when** it shrinks: a short line must not
open punctuation gaps further, and a comfortable line must keep the ADR-0124
baseline. Only an overfull (negative-adjustment) line may spend the reserve.

## Decision

1. **Every punctuation-adjacent boundary gets a small shrink reserve.**
   `soma-layout::apply_punctuation_spacing` first computes the ADR-0124
   `pair_squeeze`, then adds ordinary `Glue::shrink`:

   ```text
   gap      = ink_right(left) + ink_left(right)      (or class fallback)
   pair     = clamp(gap - pair_target, 0, pair_cap)  (0 if not a pair)
   reserve  = clamp(gap - pair - min_gap, 0, extra_cap)
   boundary.glue.shrink += reserve
   ```

   Full-width: `extra_cap = 0.125em`, `min_gap = 0.0625em`.
   Half-width: `extra_cap = 0.0625em`, `min_gap = 0.03125em`.
   `Dash` (`… — – ·`) and non-punctuation boundaries get no reserve.

2. **The reserve is ordinary glue shrink.** The breaker already adds
   `boundary.glue.shrink` to the line's shrink total, so the reserve is used
   only when `effective > target` (a justified line with a negative adjustment)
   and is distributed proportionally with every other shrinkable boundary. It
   never touches the stretch path, so a short line cannot become tighter.

3. **The reserve cannot make the outlines touch.** The cap subtracts the
   ADR-0124 `pair_squeeze` and a minimum residual ink gap. A table with
   measurements and a table without both stay above the residual.

4. **No new DP state and no new cost term.** The shrink total is already part of
   the `(i,j)` evaluation; the ratio/badness sees the larger capacity. The
   reserve is a rule, not an independent penalty. If calibration shows that
   lines overuse it, a later ADR can add a punctuation-shrink amount or
   priority term; the geometry is already in place.

5. **Versions.** `model_version` 6 → 7 (the set of feasible breaks and line
   adjustments changes) and `rules_version` 3 → 4 (the reserve constants and
   the pair/shrink interaction). The canonical schema id is unchanged: the
   result is still `naturalWidth`, `trimLeft/right`, `adjustment` and box
   coordinates. `engineHash` already includes model/rules versions.

## Consequences

- A line that is a little too wide can close its punctuation bearings instead
  of becoming overfull, moving to a much worse break, or paying the emergency
  term. This is the largest visible gain on punctuation-dense Chinese text.
- A comfortable line is unchanged: `pair_squeeze` is still the only
  punctuation adjustment, and the reserve is invisible unless the line needs
  negative adjustment.
- A line with several punctuation boundaries can recover more width than a line
  with one. That is intentional: those are exactly the gaps that were empty.
- `compress_count` still applies only to **edge** trim. The dynamic reserve is
  not counted as a trim event; it is glue. A later calibration could split the
  two if the corpus shows overuse.
- The pair baseline and the reserve are capped independently but the residual
  is shared, so the two cannot double-spend the same bearing.
- `Line.adjustment` becomes more negative on lines that use the reserve; the
  renderer already applies it through box positions.
- `LineTrace.pair_squeeze` still reports the baseline pair only; the extra
  reserve is part of `shrink` and the actual negative adjustment, not a separate
  field.

## Alternatives considered

- **Make punctuation shrink always active as baseline spacing.** Rejected: a
  short line would either ignore it (no effect) or, worse, a compositor might
  apply it symmetrically and close gaps that should stay open. The baseline is
  the pair target; the reserve is for overfull lines only.
- **Prioritize ordinary glue before punctuation shrink.** A sensible future
  refinement, but it requires a second distribution class and its own
  calibration. The first version uses the existing proportional shrink; the
  reserve is small enough that ordinary glue still receives its share.
- **Price punctuation shrink with a separate weight.** Deferred. The badness
  ratio already punishes shrink, and the user-facing goal was to make the
  reserve available; adding a price now would need corpus evidence and another
  calibration pass.
- **Extend the reserve to ragged last lines.** Rejected for this round: a
  ragged line has no right text edge and ADR-0036 deliberately keeps it from
  stretching; an overfull ragged line still goes through trim/fallback. The
  common overfull case is a justified body line.
