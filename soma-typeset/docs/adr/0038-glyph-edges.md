# ADR-0038: Every glyph edge, not just a mark's

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0036 (line-end trim), ADR-0037 (punctuation ink),
  ADR-0006 (determinism), `docs/font-preset.md`, `docs/mixed-font.md`
- Touches: `soma-metrics` (`soma.metrics/3`), `tools/metrics-gen`,
  `soma-layout` (`scale_trim`), `soma.layout/6` canonical values

## Context

ADR-0036 made a justified line pay only for the ink of its last mark, and
ADR-0037 fed it the real outline. Both stopped at **punctuation**: the rule
table only gives a mark a non-zero `compress_right`.

Every other glyph has a bearing too. Measured on MiSans Regular, Han ink fills
0.861 em of a 1 em advance, so a Han character leaves ~0.07 em of air on each
side. A line ending in a Han character therefore ends 0.07 em short of the text
edge — small, but it is the same error ADR-0036 removed for marks, and it makes
"lines ending in a mark" and "lines ending in a character" sit on two slightly
different edges.

With fallback (`docs/mixed-font.md`) the gap becomes visible: a line whose last
atom comes from a rescue face must trim by **that face's** bearing, which may
differ from the body face's by several percent of an em.

One tempting generalisation must be rejected here. It is *not* correct to charge
every glyph by its ink when deciding line width:

> A Han advance is a **cell**, not an ink box. Thirty characters occupy thirty
> em whether the face draws them fat or thin. Measuring the line by ink would
> let the breaker place 35 characters into a 30 em measure and overflow the
> block.

The 8.8-point spread of `ink/advance` across the example book's justified lines
(`docs/font-preset.md` §1) is therefore a fact about how dense the text *looks*,
not a licence to change the measure. Line width stays advance-based; what
becomes universal is the **edge treatment**.

## Decision

1. **`soma.metrics/3` records both bearings for every glyph**, not only for
   marks:

   ```text
   per face:
     ... version 2 sections ...
     u32 glyph_ink_count                      -- sorted by codepoint
     per entry: varint(codepoint delta) | u8 ink_left | u8 ink_right
   ```

   Bearings are quantised to `u8` (16 units of 1/4096 em, ~0.4% em). At 10.5 pt
   that is under 0.005 px of error at 300 dpi — far below the 1/4096 resolution
   the rest of the engine already accepts. Version 1 and 2 blobs still parse;
   they simply report no dense ink, and the rule table's constants remain the
   fallback.

2. **`scale_trim` applies the measured bearing to every atom**, not only to
   atoms whose class is punctuation:

   ```text
   compress_right = ink_right(face, atom)
   compress_left  = ink_left(face, atom)
   ```

   The breaker needs no change. It already trims by `compress_right` on a
   justified line and clamps it to the atom's advance (ADR-0036); ADR-0037
   already fed it a measured value. This ADR only widens *which atoms* have one.

3. **Line width stays advance-based.** `natural`, `effective`, `adjustment`,
   `ratio`, `badness` and the DP recurrence are unchanged. No schema change to
   `soma.layout`; `Line.natural_width` keeps meaning "sum of advances and glue".

4. **The trim is per face, not per class.** A fallback atom carries its own
   face's bearing; a line ending in a rescue face's glyph trims by that glyph's
   edge. This is the "one algorithm for marks and fonts" requirement, and it is
   why ADR-0038 has to precede the fallback work in `docs/mixed-font.md`.

5. **No new engine parameter.** `trim_priority: EndFirst` still decides the
   order when a line is overfull; the bearing is data, not policy.

## Consequences

- Every justified line gains `~0.07 em` of accounted width (the trailing Han
  bearing), so `adjustment` grows and `ratio` moves slightly. Breakpoints can
  shift; the test suite and the corpus report will show by how much.
- `soma.layout/6` canonical values change (`trimRight`, `adjustment`, the last
  box's `advance`, and the `x` of nothing — earlier boxes do not move). The
  schema id does not change: shape and meaning are identical.
- A CFF face keeps `compress_right = 0` unless the rule table gives it a
  constant, because it has no outlines. It behaves exactly as before this ADR.
- Blob size: per-glyph edges cost roughly three bytes each — a one-byte codepoint
  delta plus two bearings. A body face grows from 33 KB to about 150 KB; the
  committed LXGW baseline is 360 KB for 46,473 glyphs across three faces (it was
  690 KB when the codepoint was a fixed `u32`). A coarser one-byte ink-width
  encoding was rejected because it loses the left/right split that trailing and
  leading trim each need.
- Measured on the example book: still 11 pages, `canonicalHash` moves to
  `a94a8aee18fd0d37`. Every justified line now satisfies
  `boxes.last().x + advance == indent + target_width`, and at 300 dpi the ink
  edge of a Han-ending line moved from 1554 to 1557 against a text edge of 1559
  (a mark-ending line sits at 1558).
- A rescue face measures and renders with its own bearings: the mixed-font probe
  (`𪚥` absent from MiSans, present in LXGW WenKai) embeds two subsets,
  `SomaSerif` and `SomaSerif-Fallback`, and the glyph is no longer `.notdef`.

## Alternatives considered

- **Charge every glyph by ink when measuring the line.** Rejected above: the
  advance is a cell, and breaking on ink overfills the measure.
- **Keep trim for marks only.** Rejected: it leaves two different text edges,
  and it cannot express a fallback face whose bearing differs.
- **Store `ink_width` only and assume symmetric bearings.** Rejected: Chinese
  marks and punctuation are deliberately asymmetric; the asymmetry is the whole
  point.
- **Read the outline at layout time.** Rejected by the architecture: the core
  never opens a font.
