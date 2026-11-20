# ADR-0037: Punctuation ink comes from the outline, not from a class constant

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0036 (line-end trim), ADR-0006 (determinism), ADR-0022 (bold
  faces), `docs/params.md` §10.3
- Touches: `soma-metrics` (`soma.metrics/2`), `soma-core` (`Metrics::ink_right`),
  `soma-layout` (`scale_trim`, `TableMetrics`, `StyledMetrics`),
  `tools/metrics-gen`

## Context

ADR-0036 made a justified line pay only for the ink of its last mark, using
`compress_right` from the rule table: `1/2em` for full-width marks, `1/8em` for
half-width ones. That is a **class average**. Measured against MiSans, the real
gaps between ink and advance are:

| Mark | Rule constant | Real `ink_right` |
| --- | --- | --- |
| `。` | 0.5em | 0.617em |
| `，` | 0.5em | 0.703em |
| `？` | 0.5em | 0.548em |
| `,` | 0.125em | 0.073em |

A line ending in `。` or `？` lands close to the edge; a line ending in `，`
keeps about 0.2em of air, which is the case the preview exposed. The gap is a
property of the **font**, so no rule constant can close it for every face.

The measurement itself is cheap: a TrueType glyph's bounding box is four
integers in its `glyf` header, reachable from `loca`. The tool already walks
`cmap`/`hmtx`; it did not walk `glyf` because nothing needed the outline.

## Decision

1. **`soma.metrics/2` adds an ink section per face.**

   ```text
   u32 ink_count
   per ink: u32 codepoint | i16 ink_left | i16 ink_right
   ```

   The gaps are in 1/4096 em, measured from the advance box to the outline
   bounding box, and may be negative when a glyph overhangs. Entries are sorted
   by codepoint, like advances. A version 1 blob parses and reports no ink at
   all, so old tables keep working with the rule constant.

2. **Only punctuation carries ink.** `tools/metrics-gen` records the ASCII
   punctuation blocks, general punctuation, CJK symbols, CJK compatibility
   forms, and the halfwidth/fullwidth forms. Recording a bbox for every Han
   glyph would triple the table and change nothing: Han ink fills its advance.

3. **`Metrics::ink_right` is an optional seam.** The default returns `None`.
   `TableMetrics` and `StyledMetrics` answer from the table and scale by the
   style's font size, exactly like `advance`. A source without outlines (CFF
   fonts, `SyntheticMetrics`, test doubles) keeps the default.

4. **`scale_trim` prefers the outline.** After scaling the rule caps to the
   atom's size, a mark whose outline is known takes `compress_right = ink_right`.
   The breaker still clamps the trim to the atom's own advance, so a bad table
   cannot produce a negative advance.

5. **No rule table changes.** `compress_right` stays the fallback and the
   documented `ParamOnly` value. The ink is measurement, not policy.

## Consequences

- On MiSans, the comma line from the preview now ends at 1558/1749 px at
  300 dpi against a text edge of 1559; a Han line ends at 1554 (Han ink has a
  small natural bearing). Before this ADR the comma line ended at 1540.
- The example book still has 11 pages. Its PDF bytes change because the table
  and the trims change; `book.json` (rendered without `--metrics`) still uses
  the embedded version 1 table and is unchanged.
- `metricsHash` changes for any face rebuilt with ink, which is exactly the
  signal the canonical page result is supposed to carry.
- Blob size: MiSans grows from 31,188 to 33,460 bytes for 231 punctuation
  glyphs; the math face adds 52 more.
- A CFF font still gets no ink. `docs/debt.md` D84 stays closed for `glyf`
  fonts and the fallback is documented.

## Alternatives considered

- **Record ink for every glyph.** Rejected: ~20k Han glyphs × 8 bytes for a
  bearing that is always zero.
- **Store the ink gap as a style parameter per mark class.** Rejected: it is a
  font property, not a design decision; a template would have to be rewritten
  for every face.
- **Read the outline at layout time.** Rejected by the architecture: the core
  never opens a font, and `soma-core` must stay `no_std` and target-independent.
