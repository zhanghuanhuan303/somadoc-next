# ADR-0073: CFF faces record punctuation ink

- Status: Accepted
- Date: 2026-11-21
- Related: ADR-0037 (ink-aware trim), ADR-0038 (any atom can end a line),
  ADR-0051 (the trim constants are a budget), D96 (the embedded table)
- Touches: `tools/metrics-gen` (new CFF bbox interpreter, punctuation ink for
  CFF faces), `soma.metrics` (same version, more populated data)

## Context

A justified line drops the last glyph's right side bearing so the ink lands on
the text edge. For a face whose metrics table carries outline ink, that bearing
is exact (ADR-0037). For everything else the engine falls back to the rule
table's constant: half an em for full-width marks.

Most Chinese body faces are CFF: Source Han Serif/Sans, Noto CJK, Fandol. A CFF
face has no `glyf` table, so `metrics-gen` recorded no ink at all and every line
ending on a comma used the constant. Measured on the default `song.tex` sample
with Source Han Serif at 10pt:

| line | ink right edge | measure edge |
|---|---|---|
| `…已经记录，` | 465.78 pt | 468.56 pt |
| `…类别与标点` | 468.00 pt | 468.56 pt |

The comma's ink stops 2.8 pt (0.28 em) short of the measure while the next line
ends flush. That is the visible "notch" a reader reports as "the break looks
pulled in". The comma's real right bearing in Source Han is 0.82 em, not 0.50 —
the constant is simply wrong for that face, and `docs/trim-calibration.md`
already measured 0.627 em across a different sample.

Changing the constant is not a fix: MiSans' comma bearing is 0.30 em, so one
number cannot serve both faces (and ADR-0051 rejected re-calibrating it because
it moved the corpus). The data has to be per glyph.

## Decision

### 1. `metrics-gen` measures CFF outlines

A bbox-only Type 2 interpreter (`tools/metrics-gen/src/cff.rs`) parses the CFF
INDEX structure, `FDSelect`/`FDArray` and the private dicts, runs one charstring
per glyph, and accumulates the extents. Curves are sampled at their extrema, not
at their control points: a control point can sit outside the ink, and a trim
that overshoots pushes a mark into the margin.

It is deliberately not a general interpreter: no hints beyond mask skipping, no
outline output, no rewriting, unknown `12 x` operators clear the stack.

### 2. CFF faces record punctuation ink, not per-glyph ink

A `glyf` face gets a row for every codepoint (`glyph_ink`), because its bbox is
free to read. A CFF face would pay a charstring run per glyph and a table row
per glyph: regenerating the embedded sample this way took the blob from 84 KB to
1.1 MB. So a CFF face keeps the compact v2-style `ink` list, which covers
punctuation only.

Consequence: for a CFF face, punctuation gets an exact bearing and a Han
character at a line end keeps the class constant (zero). A Han glyph's bearing is
about 0.02 em, so the difference is invisible; a comma's is 0.82 em, and that is
the whole point.

`Face::ink()` already consults `glyph_ink` first and then `ink`, and `Face::has()`
uses `glyph_ink` as its authoritative coverage list only when it is non-empty —
so a punctuation-only list cannot corrupt coverage.

### 3. The table version does not change

`soma.metrics/5` already carries both sections. A regenerated table has different
bytes and therefore a different `metricsHash`, which is the intended signal: the
layout changed because the measurement changed.

## Consequences

Regenerating the sample tables (`scripts/song.sh`, `scripts/fonts.sh`,
`tools/metrics-gen` by hand) fixes the line end:

| line | before | after |
|---|---|---|
| `…已经记录，` | 465.78 pt | 468.96 pt |
| `…类别与标点` | 468.00 pt | 468.00 pt |

The comma now hangs to the text edge instead of stopping 2.8 pt (0.28 em) short.
The Han character on the next line keeps its natural bearing — a CFF table has
no per-glyph ink, so a line ending on a Han character is unchanged — and the two
edges now differ by 1 pt at 10 pt type, which is the conventional hanging look
rather than a notch.

### A negative gap is not a trim

Some marks overhang their advance box, so `advance - x_max` can be negative.
`soma-layout` used to copy `ink_left`/`ink_right` straight into
`compress_left`/`compress_right`, which the IR validates as non-negative — a
table with an overhanging mark made the whole paragraph fail with
`BreakError::Ir`. Both sides now clamp at zero: the generator when it writes the
table, and `scale_trim` when it reads one (a trim into the glyph is never what a
line end wants).

Two tests in `tools/metrics-gen` pin the interpreter against the committed CFF
fixtures: the CID-keyed and name-keyed faces both draw axis-aligned rectangles,
so the expected ink is exact (`(0,0,500,700)`, `(0,0,600,700)`, `(0,0,1000,800)`)
and any regression in INDEX parsing, `FDSelect`, subroutine bias or the curve
math shows up immediately.

The committed `crates/soma-metrics/data/noto-cjk.smt` is a **version 1** blob
with no ink section at all; regenerating it at version 5 is a separate decision
(1.1 MB, and the corpus baseline moves). It is recorded as D96.

## Alternatives

- **Re-calibrate `CLOSE_TRIM_RIGHT` to 0.627 em.** Fixes Source Han, over-trims
  MiSans and every other face by 0.33 em, and moves the corpus. Rejected.
- **Per-glyph ink for CFF.** Most accurate, but 1.1 MB of table for a bearing
  that only matters on punctuation. Rejected.
- **Compute the bearing at layout time.** The layout has no font by design
  (ADR-0007); the seam is the metrics table. Rejected.
- **Shift the glyph in the renderer.** Would leave SVG, HTML and the browser
  slice disagreeing with the model. Rejected.

## Verification

```sh
cargo test --manifest-path tools/metrics-gen/Cargo.toml   # 4 tests
bash scripts/song.sh                                      # regenerates the CFF table
cargo test --workspace                                    # 780 tests, unchanged
bash scripts/gates.sh                                     # 12 gates, baseline unchanged
```
