# Proposal: one ink model for marks, fonts and fallback

**Status**: F1 (metrics v3), F2 (profile), F4 (rescue faces) and F5
(validation) are implemented. F3 as originally written — charging the line by
ink — was **rejected** by ADR-0038: a Han advance is a cell, and breaking on ink
overfills the measure. What shipped instead is the universal edge treatment in
ADR-0038 plus per-face bearings for fallback.
**Related**: ADR-0006 (determinism), ADR-0036/0037 (line-end bearing, ink),
ADR-0038 (glyph edges), `docs/fonts.md`, `docs/mixed-font.md`.

## 1. The problem in one number

The engine charges a line for the **advance** of every atom. Advance is not
ink:

| Face | Han ink / em | Mark ink / em |
| --- | --- | --- |
| MiSans Thin | 0.839 | 0.173 |
| MiSans Normal | 0.861 | 0.233 |
| MiSans Heavy | 0.904 | 0.343 |
| LXGW WenKai | 0.906 | 0.241 |

Measured on 256 sampled Han glyphs and 13 full-width marks (`--audit`, columns
`han_density`, `punct_density`).

Within one face the error is not a constant offset, because lines differ in
composition. Across the 68 justified lines of the example book:

```text
ink/advance mean 0.804, std 0.018, min 0.753, max 0.841
loosest  29 atoms, ink 21.8 em, ratio 0.753
densest  31 atoms, ink 26.1 em, ratio 0.841
```

Every one of those lines is "full" to the DP. Visually the loosest is 11.7%
emptier than the densest. That is the error worth removing.

## 2. Why not one Rust engine per font

Because it does not scale and cannot be audited: there is no finite font list,
behaviour would live in branches instead of data, and two faces with identical
measurements would still get two code paths.

**One measurement engine + one deterministic derivation + one data record per
font** adapts to every face, including one released tomorrow, and stays
diffable.

## 3. The unified model

Every difference between "how wide a line is" and "how wide it looks" is a
**bearing**: empty space inside a glyph's advance box. Marks, Han, Latin,
accented glyphs and fallback faces all have it. So stop treating the line-end
mark as a special case and charge **ink plus designed gaps** everywhere.

For a line covering atoms `[i, j)`:

```text
ink(k)          = advance(k) - bearing_left(k) - bearing_right(k)
natural_gap(k)  = bearing_right(k) + bearing_left(k+1)
designed_gap(k) = rule(script(k), script(k+1))      -- 0 for Han-Han
visual(i,j)     = Σ ink(k) + Σ [designed_gap(k) - natural_gap(k)]
```

The bracket is the **boundary adjustment**: negative when the font already left
more air than the design wants, positive when two scripts need more room than
their bearings provide.

### Four cases, one primitive

| Case | `designed_gap` | `natural_gap` | Adjustment | Equals today's |
| --- | --- | --- | --- | --- |
| Han next to Han, same face | 0 | `right + left` | `-(right+left)` | ink accounting |
| Line end | — | `bearing_right(last)` | `-bearing_right(last)` | **line-end trim (ADR-0036)** |
| Line start | — | `bearing_left(first)` | `-bearing_left(first)` | leading trim |
| Han next to Latin | `CJK_LATIN_GAP` | `right + left` | `gap - bearings` | boundary glue |
| **Two different faces** | by script pair | `right(faceA) + left(faceB)` | automatic | **mixed typesetting** |

The line-end trim does not need its own rule any more: it is the boundary
adjustment of the **virtual** boundary after the last atom. The same holds for
the line start. One formula, four behaviours, and no `trim_priority` flag.

### Mixed fonts fall out for free

A fallback run makes atom `k` draw with face B inside a line set in face A. The
model only needs `bearing_*(k)` to come from **the face that draws atom k**:

- the gap between A and B is normalised to the script pair's design gap, so a
  fallback glyph does not open a hole;
- a line ending in a fallback face's mark trims by **that face's** bearing, not
  the body face's — which is exactly the "different font width at the line end"
  problem, solved by the same expression;
- line height becomes the max over the faces actually used (already the K3 rule).

No new mechanism. This is why the unified model is the right home for fallback
rather than a second, parallel code path.

### Determinism and cost

`ink` and `natural_gap` are integers from the metrics table; `designed_gap` is
an integer from the rule table. `visual` is a prefix sum, so `eval_line` stays
O(1) and the DP stays O(n²) with exactly today's shape. No floating point, no
font access at layout time.

## 4. What a font may and may not decide

| Parameter | Decided by | Why |
| --- | --- | --- |
| advance, bearings, x-height, cap-height | **font** (measured) | facts about the file |
| line-end trim | **font** (falls out of §3) | no longer a rule |
| visual line width | **font** (falls out of §3) | see §1 |
| CJK/Latin design gap | **rule table** | a Chinese typesetting convention, not a font property |
| line height | **font**, template may override | measured |
| `line` penalty, `badness`, `tolerance`, `short_last_*` | **designer** | how the book punishes looseness |

A new font changes geometry only. If it also moved `line` and `badness`,
"switch the body face" would become "switch the typographic taste".

## 5. What has to be measured

`soma.metrics/3` must carry, **per glyph**, both bearings — not just for marks:

```text
per face: u32 ink_count
per ink:  u32 codepoint | i16 ink_left | i16 ink_right
```

The version 2 section already has this shape; it just covers 13 marks. Version 3
extends the range to every glyph the face has, in the same compact form. A
20k-glyph face adds ~160 KB uncompressed; the alternative is a coarser encoding
(one byte of `ink_width / advance` per glyph, ~20 KB) plus a x-height class,
which is enough for the width model but not for exact trim. Both are integers
and hashable.

For CFF faces there is no outline data, so `derived: false`: the breaker keeps
today's constants and the line is measured by advance. A font without outlines
simply has no profile — and `docs/mixed-font.md` already excludes CFF faces from
rescue duty for the same reason.

## 6. Staging

| Stage | Deliverable | Risk | Gate |
| --- | --- | --- | --- |
| F1 | metrics v3: bearings for all glyphs (or the coarse form) | low | v1/v2 tables still parse; no DP change |
| F2 | `FontProfile::derive` + printed report | low | deterministic; no DP change |
| F3 | `visual()` in the breaker; remove the trim special case | **high** | `soma.layout/7`; every golden moves |
| F4 | fallback chain feeding `face(k)` | high | depends on `docs/mixed-font.md` P1/P2 |
| F5 | calibration: stretch/shrink now apply to ink gaps | medium | density spread down, pages stable |

F1+F2 ship without moving a breakpoint. F3 is the decision point.

## 7. Validation

1. **Density spread** (the §1 metric) must fall from 8.8 points to under 2.
2. **Breakpoint drift**: a well-behaved face should move few breaks; a very
   light face should move towards more characters per line.
3. `K-P ≤ Greedy-C` still holds; the brute-force oracle uses the same model.
4. The example book stays at 11 pages, or the change is explained.
5. Mixed-font check: a line with a fallback glyph must keep the same visual
   density as its neighbours — measured, not asserted.

## 8. The first thing to build

F1 and F2 only, plus a report that prints, for a given book, the per-line
`visual / advance` ratio before and after. That report costs nothing, changes no
breakpoint, and turns §1 from a static measurement into evidence about the
actual document. If the drift is small on the real corpus, F3 can wait; on the
example book it is already 8.8 points.
