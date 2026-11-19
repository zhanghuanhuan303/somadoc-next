# ADR-0030: Math completion — fences, matrices, math face, text style

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0026 (display math), ADR-0027 (inline math), ADR-0012 (metrics
  table), `docs/debt.md` D71/D72
- Touches: `soma-metrics` (math face), `soma-core` (math metrics hooks),
  `soma-math` (fences, matrices, text-style constants)

## Context

ADR-0026 shipped a working display-math box with four explicit gaps (D71): no
stretched delimiters, no matrices or alignment, formula symbols taken from the
body font, and inline math reusing display constants (D72). All four are common
in Chinese technical writing, and two of them (delimiters, matrices) decide
whether a formula reads at all.

## Decision

### Delimiters are laid out as a unit

`\left…\right` becomes `Node::Delimited { left, body, right }`. The parser scans
to the matching `\right` (tracking nested `\left`), so the body can contain any
node, including matrices. Layout measures the body, then:

- picks the smallest delimiter size whose `ascent + descent` covers the body,
  capped at 4× the base size, rounding up and nudging until the measured extent
  really covers it (every lookup truncates, so the ratio alone can be one unit
  short);
- centres the delimiter box on the body's vertical centre:
  `baseline = (depth − height)/2 + (ascent − descent)/2`;
- `.` is the null delimiter and advances nothing.

`\right` without `\left` keeps the delimiter as a symbol instead of dropping it.

### Matrices and alignment

`\begin{matrix}` / `pmatrix` / `bmatrix` / `vmatrix` / `cases` / `aligned`
(also `align`, `split`) parse into `Node::Matrix { kind, rows }` where
`rows[row][column]` is a node list. The body is split on top-level `\\` and `&`,
respecting nested environments and braces, so a nested matrix or `\text{a & b}`
never splits its parent.

Layout measures every cell at the base size, then:

- column width = max cell width; row ascent/descent = max over the row;
- row baselines: the first row sits on the formula baseline, each next row is
  placed below the previous row's descent plus `frac_gap`;
- alignment: centred for matrices, left for `cases`, and for `aligned`
  alternating right/left around the `&` columns (the TeX convention);
- fences use the same code as `\left…\right`.

Unknown environments are errors (`UnsupportedCommand("begin{...}")`), and a
mismatched `\end{...}` is `UnbalancedEnvironment`.

### A dedicated math face

The metrics blob gains a fifth face, `family = 2` (`math`), generated from Noto
Sans Math. The generator's enumerated range extends to U+1D7FF so math
alphanumerics are covered.

Two defaulted `Metrics` hooks carry it:

```rust
fn math_advance(&self, text: &str, style: StyleId) -> Width;   // default: advance
fn math_metrics(&self, style: StyleId) -> LineMetrics;         // default: line_metrics
```

`TableMetrics` and `StyledMetrics` answer from the math face at the style's font
size; adapters without one keep the body font. Effect: `∑`, `∫`, `√`, Greek and
relations get their real advances and extents, which is what makes formula
widths and small-formula line boxes correct.

The blob is `soma.metrics/1` unchanged; `VERSION` stays 1, and the `metricsHash`
in the page result changes because the bytes changed.

### Text style gets its own constants

`MathStyle` gains `text_frac_scale` (700), `text_frac_gap` (80) and
`text_big_scale` (1100), used when `display == false`. Display fractions now
scale their numerator/denominator at **full size** (1000) — TeX's `\textstyle`
inside `\displaystyle` — while inline keeps script size. Big operators shrink in
text style and keep their limits to the right (already true via `display`).

## Consequences

- `\left( \begin{aligned} a &= b \\ c &= d \end{aligned} \right)` typesets with
  fences that cover the block.
- Formula widths change wherever a symbol came from the body font; the corpus
  `math-page` jitter p95 moved from 435 to 452 and page/line counts did not
  change.
- `soma-core` gained two defaulted methods; no DP or cost change, and every
  adapter without a math face is byte-identical.
- The committed metrics blob grows from 63,212 to 84,720 bytes; the wasm
  fixture hash changes accordingly.
- Remaining math debt narrows to D71's leftovers (no `\overbrace`, `\color`,
  `\begin{array}{...}` column specs, no automatic `\left` *body* reflow, no
  \underbrace) and D72's (no italic math font, no per-script kerning).

## Alternatives rejected

- **Body font for math.** Cheaper, but the radical and the summation are the
  wrong width and extent, and `√x` visibly separates from its bar.
- **Stretch delimiters by repeating glyphs.** Needs font-specific assembly
  recipes; a single scaled glyph is what the metric table can support.
- **Column specs (`{cc}`) for matrices.** Real TeX parses them; nothing in the
  corpus needs them yet, and `aligned` covers the alignment case.
- **Inline math with display constants.** Makes inline formulas taller than the
  line rhythm they live in (D72 before this ADR).
