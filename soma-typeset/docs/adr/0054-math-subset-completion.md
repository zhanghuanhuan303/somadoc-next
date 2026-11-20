# ADR-0054: the math subset closes its gaps — italic variables, arrays, braces, colour, numbering

- Status: Accepted
- Superseded in part by: ADR-0056 §4 (a wide brace is now assembled from
  rules instead of one stretched glyph)
- Date: 2026-11-20
- Closes: D71 (with the residuals listed at the end)
- Related: ADR-0026 (math box model), ADR-0030 (math completion),
  ADR-0053 (baseline propagation), `docs/debt.md` D71/D88
- Touches: `soma-math` (layout and parser), `soma-core` (`Metrics::has_glyph`),
  `soma-template` (`numbering.equations`), `soma-layout` (equation numbers),
  all three renderers (`stretch`, `color`). Schema: `soma.pages/9` →
  `soma.pages/10`, `soma.template/9` unchanged in shape but gains a
  `numbering.equations` key.

## Context

D71 listed five holes in the math subset. They shared one root: the layout had
a single glyph per node, a single ink model (the font's ascent and descent), and
no notion of a formula that carries a number in the margin.

## Decision

### 1. Variables are italic when the face has the glyph

`Node::Symbol` is a multi-character run such as `\sin` or `\text{const}`; a bare
ASCII letter becomes `Node::Letter(char)` and is set in the Mathematical Italic
block (`a` → U+1D44E, `A` → U+1D434).

Whether the substitution happens is a *metrics* question, not a math question:
`MathMetrics::has(character)` and `Metrics::has_glyph(character, style)` both
default to `false`, and `StyledMetrics` answers from the union of the math and
body faces. A face without the italic block therefore keeps the upright letter
instead of drawing `.notdef`, which is what every test adapter and the
synthetic metrics do.

### 2. `array` takes its column formats from the source

`\begin{array}{lcr}` reads the format string before the body: `l`, `c`, `r` per
column, anything else `c`. `Node::Matrix` carries `aligns: Vec<ColumnAlign>`,
and a cell's `x` is computed from its column, falling back to the old
kind-based rule when the vector is short.

### 3. `\substack` stacks lines centred on each other

`\substack{a\\b}` becomes `Node::Substack(Vec<Vec<Node>>)`; each row is measured
and placed so that its baseline clears the previous row's depth by `frac_gap`.

### 4. Braces are placed by their ink band, not by the font ascent

`\overbrace{…}` / `\underbrace{…}` use one stretchy glyph (U+23DE / U+23DF),
grown uniformly up to 2.2× and then stretched horizontally for the remainder.
Order matters: a purely horizontal stretch flattens the hooks into a wave.

The two glyphs carry their ink in a narrow band far from the em box — measured
from Noto Sans Math as 0.656…0.893 em above the baseline for U+23DE and
0.075…0.310 em below it for U+23DF. Placing them by `metrics.ascent` put the
brace a full em above a line of lowercase letters, with a visible hole. The
band is therefore a table in `soma-math` (`brace_ink`), the body is measured
with `measure_ink`, and the brace baseline is derived so that the ink edge sits
`frac_gap` from the body's ink edge.

`\overbrace{x}^{n}` places `n` centred over the brace's ink top, not after it:
a script on a brace is a label, not a superscript of a symbol.

The same ink measure tightens the limits of a display operator, whose
`\substack` had double leading for exactly the same reason.

### 5. `\color` applies to the rest of its group

`\color{red} a + \color{blue} b + c` wraps the remainder of the enclosing group
in `Node::Color`, and the items it produced get the colour unless a nested
`\color` already set one. Named colours (`red`, `green`, `blue`, `black`,
`white`, `gray`, `orange`) and `#rrggbb` are accepted; anything else is an
error, not a silent black.

### 6. Numbered display formulas

`numbering.equations` (e.g. `"({n})"`) turns on equation numbers. A formula is
numbered when its source is an `equation`, `align`, `gather` or `multline`
environment — the `*` variants and the inner `aligned`/`split` are not. The
counter is per document, not per page.

Numbering happens in `soma-layout`, not in `soma-math`: the math box keeps
producing geometry, and the layout appends one right-aligned `MathItem` per
row. `MathLayout::rows` carries each aligned row's baseline for this purpose.
The item's `x` is relative to the box origin, so the centring offset is
subtracted; the label lands on the right margin whichever way the body is
centred.

A `aligned`/`cases` block inside a formula is one row, so it gets one number.

### 7. Renderers honour `stretch` and `color`

`MathItem` gained `stretch: u16` (permille, 1000 = as is) and
`color: Option<u32>`. PDF writes the stretch as an `x` scale in the text
matrix, SVG as `scale(1.xxx 1)`, HTML as `transform:scaleX(...)`. All three
override the fill per item.

## Consequences

- `soma.pages` moves to version 10: `MathItem` serialises `stretch` and
  `color`.
- Conditional layouts are exercised by `examples/book/song.tex`, which now sets
  an `array`, a `\substack`, an overbrace, a colour run, a numbered
  `equation`, and a numbered `align`.
- Seven new tests: italic only when the face has it; `\text`/`\sin` stay
  upright; `array` column formats; `\substack` order; `\overbrace` ink and its
  centred label; `\color` scope and unknown colour; numbered equation, one
  number per aligned row, bare formulas unnumbered.

## Still not TeX

- **The brace is one glyph.** TeX assembles an overbrace from hooks and
  extensible pieces; our font has the single-character version, so a very wide
  body gets a horizontally stretched brace with flattened hooks.
- **No `\text{}` with inline math inside**, no `\tag`, no `\notag`/`\nonumber`,
  no `\begin{cases}` numbering.
- **Italic substitution is a face property.** The sample's math face has the
  italic block; a face without it stays upright, silently and by design.
- **`\color` takes a name or hex**, not an expression, and cannot be unset
  mid-group except by another `\color`.
- The ink band constants are measured from Noto Sans Math. A different math
  face with differently drawn braces would want its own numbers; they are in
  one table for that reason.
