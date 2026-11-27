# ADR-0123: A list marker's hang is measured, not guessed

- Status: Accepted
- Date: 2026-11-28
- Related: ADR-0058 (hanging indent), ADR-0036 (trim), ADR-0041 (a change in
  feasible breaks bumps `modelVersion`), ADR-0112 (`modelVersion` 4)
- Touches: `crates/soma-layout` (list compilation, `ParagraphStyle`),
  `crates/soma-core` (`model_version`), tests, `docs/params.md`,
  `docs/kp-model.md`

## Context

A template expresses a list item as a hanging indent: the paragraph's
`first-indent` is negative, the kernel receives `hang = -first-indent`, and
line 0 starts at `-hang` and is `hang` wider (ADR-0058). The Plex template and
the fixture templates say `first-indent: -1.2em`.

That number is a guess. The marker is a real string — `•`, `1.`, `10.` — whose
advance comes from the font and whose width changes with the digit count. With
Plex, `• ` measures 1.25 em, so the first text column sat 205 units (1/4096 em)
to the right of the wrapped lines. With `10. ` the error is about an em. The
misalignment is small enough to pass a smoke test and large enough to see in a
PDF.

The marker gap is also the first line's only glue. Justification distributes
`adjustment` over the line's boundaries; with an underfull first line the gap
absorbs the whole remainder and the text moves even when the hang is right.
Zeroing the gap's stretch and shrink is not sufficient: `distribute` still
shares a remainder when the total stretch is zero, which leaves a residual
unit or two.

## Decision

1. **Measure the prefix per item.** In `soma-layout`, when a list item's first
   paragraph is compiled, `marker_width = metrics.advance("{marker} ")` with the
   paragraph's own style. This value becomes `ParagraphStyle::hang`; the
   template's negative `first-indent` only says "this is a hanging list", not
   how wide the hang is. Ordered items are measured individually, so `9. ` and
   `10. ` each get their own width.
2. **The marker gap is not adjustable.** The first `Space` boundary in the
   marked paragraph gets zero stretch and zero shrink, so justification spends
   its adjustment on the text, not on the prefix.
3. **Pin the marker and the text column after breaking.** `pin_marker_gap`
   undoes the line's optical left trim on the marker prefix and translates the
   atom after the marker gap, plus every later box on line 0, to
   `line.indent + marker_width`. The gap therefore keeps exactly its natural
   width, the marker sits on the hang column whether the first line was
   justified and trimmed or ragged, and the first text glyph and every wrapped
   line share an absolute x.
4. **Descriptions keep no hang.** A description term (`item[label]`) has no
   marker and no hanging indent; `suppress_hang` still forces `hang = 0`, so
   the measured value does not apply there.
5. **Version bump.** `model_version` 4 → 5. The first line's measure changes
   for every list paragraph, so breaks and `totalCost` may change even with the
   same template and parameters. `rulesVersion`, `metricsVersion`, the canonical
   schema and the renderers are unchanged.

## Consequences

- A bullet, `1.` and `10.` all put their text on the wrapped-line column. The
  alignment is exact in integer units, not "within a pixel". The marker itself
  stays on the hang column: a wrapped item's first line is justified and would
  otherwise trim the bullet's left bearing and make it sit a fraction of an em
  left of the unwrapped items above it.
- The first line is wider by the true marker width rather than 1.2 em. This can
  move a break by one atom; that is why `modelVersion` moves. `paramHash` and
  every canonical result move with it; no renderer reads the field, so SVG and
  PDF geometry only change where a break actually moved.
- `soma-layout` now calls `Metrics::advance` once per list item. The cost is
  negligible next to the paragraph's own shaping and is already paid for the
  marker text itself.
- A template that wants a wider or narrower gutter between marker and text can
  still express it, but through padding on the marker, not by lying about the
  hang: the hang is now always the prefix's advance.
- The fixture tests assert the measured width, not 1.2 em, and check both the
  bullet and a two-digit ordered marker. A fixture whose marker happens to be
  1.2 em wide would have hidden the bug.

## Alternatives considered

- **Widen `first-indent` in the templates until it looks right.** Rejected:
  the correct value is font-dependent and digit-dependent, and a template has
  no way to measure.
- **Tell `build_boxes` to skip the marker boundary when distributing.** Rejected:
  the breaker is frozen and script-agnostic; it does not know what a list is.
  The layout layer already owns the marker, so it owns the correction.
- **Only zero the gap's stretch/shrink.** Rejected: measured residual of one
  unit with `SyntheticMetrics` and a wider residual with real fonts, because
  `distribute` shares remainders even when the total stretch is zero.
- **Emit the marker as a separate box outside the paragraph.** A larger change
  to the block model for the same visual result; the paragraph currently owns
  the marker so it can share the first line and the first-line measure.
