# ADR-0056: tags, text-mode maths, and braces that do not flatten

- Status: Accepted
- Date: 2026-11-20
- Closes: the residual list of D71 (see ADR-0054)
- Supersedes: ADR-0054 §4 (a brace is one stretched glyph)
- Related: ADR-0026, ADR-0030, ADR-0053, ADR-0054
- Touches: `soma-math` (parser and layout), `soma-layout` (`math_item`).
  No schema change: tags arrive as ordinary `MathItem`s.

## Context

ADR-0054 closed D71's five named gaps and left four residuals:

1. `\text{}` could not contain maths.
2. No `\tag`, `\tag*`, `\notag`.
3. Rows of a nested `aligned`/`cases` added numbers of their own, so an
   `equation` wrapping an `aligned` produced one number per inner row.
4. A brace was one glyph grown to 2.2× and then stretched sideways; past about
   three times that stretch the hooks became ramps, and the exact shape was
   whatever Noto Sans Math drew — the ink band had to be a table of measured
   constants.

## Decision

### 1. A tag names a row; a `\notag` leaves it alone

`\tag{...}`, `\tag*{...}`, `\notag` and `\nonumber` become `Node::Tag` /
`Node::NoTag`, which paint nothing. `matrix` collects them per row into
`RowNumber { Auto, Suppressed, Tag { text, bare } }`, and `MathLayout::rows`
carries one per aligned row. A tag written outside any matrix — directly in an
`equation` body is inside one, but a `\tag` in a bare `\[…\]` is not — sets
`MathLayout::number` instead.

Consequences of that shape:

- **Tags cost no number.** `\tag` and `\notag` leave the counter alone, so the
  next automatic number is unchanged. This matches amsmath and keeps the
  numbering stable when a tag is added.
- **A tag is text with `$…$` allowed**, which is exactly `\text{}`, so
  `soma-layout` lays a tag out by wrapping it in `\text{…}` and running it
  through `display_inline`. `\tag{A}` prints `(A)` upright; `\tag*{A}` prints
  `A`; `\tag{$\alpha$}` prints `(α)`.

### 2. Only the outermost block owns rows

`Context` counts how many `matrix` bodies enclose the node being laid out.
Rows are recorded only when that count is zero, the kind is `Aligned`, and no
rows exist yet. An `aligned` inside an `equation`, a `cases` inside an
`aligned`, and a `matrix` inside a fraction all lay out normally and add
nothing.

### 3. `\text{}` parses `$…$`

`\text{if $x>0$ then}` compiles to a text run, the maths nodes of `x>0`, and a
second text run, all at the current size. `\$` is a literal dollar sign.
Variables inside the dollars get the same italic treatment as anywhere else.

### 3a. A face is chosen per character, in both measurement and painting

`\text{行宽}` inside a formula is drawn from the **text** face: the math face is
Latin, and it has no Han. Two places have to agree about that or the block is
mis-measured:

- `StyledMetrics::math_advance` sums per-character advances, taking the math
  face when it has the character, then the style's face, then the rescue face.
- `soma-render-pdf` picks the same face per character and draws contiguous
  runs of one face, advancing by each glyph's own width. Only `stretch != 1000`
  keeps the single-face path, because only a brace stretches and a brace is one
  glyph.

The subset collector walks the same rule, so a Han glyph inside a formula ends
up in the body face's subset rather than as `.notdef`.

### 4. A brace keeps its shape at any width

The brace has two forms, chosen by `BRACE_GLYPH_STRETCH` (2000‰):

- **Narrow**: the font's brace glyph, grown uniformly up to 2.2× and then
  stretched sideways. Its ink band is the measured table from ADR-0054.
- **Wide**: assembled from rules — a flat run, two hooks and a cusp, the pieces
  TeX assembles a brace from, drawn here because the font carries only the
  single characters U+23DE/U+23DF. The hook is a quarter circle (vertical
  tangent at the tip, flat where it meets the bar) computed with an integer
  square root; the cusp is a straight-sided peak. Stroke is `size/11`, hook
  `size/4`, cusp `size/5`, all clamped to the box.
- Each slice is one rule spanning the level range of both of its ends. Ribbons
  that only followed the level at their left edge broke apart where the curve
  was steep, because the tangent at a hook tip is vertical.

The assembled form knows its own extents, so the ink band is exact for it and
the label still lands centred over the brace.

### 5. The glyph path now respects its baseline

`brace()` laid the glyph out at `-height - gap` regardless of the baseline it
was given, so a brace inside a numerator ignored the fraction and drew at the
formula's baseline. ADR-0053 made every node place itself at the baseline it
receives; this was the one arm that had been added after that rule and missed
it. It now adds `baseline` to both the glyph and the assembled form.

## Consequences

- `\begin{equation}\begin{aligned}…\end{aligned}\end{equation}` prints one
  number, and `\begin{align}…\end{align}` one per row.
- Numbered rows still span the measure so the label sits on the right margin;
  a formula whose rows are all `\notag` shrinks back to its own width.
- Nine new tests: text-mode maths, tag and star tag, `\notag`, tags not
  consuming numbers, one number for a nested `aligned`, a wide brace made of
  rules, a narrow brace that keeps its glyph, a tag carrying maths, and a brace
  inside a fraction. `soma-layout/tests/metrics.rs` adds one for the face
  switch: Han inside a formula measures with the body face, `∑` with the math
  face.
- `examples/book/song.tex` sets a tagged line, a `\notag` line, a text-mode
  sentence and a wide brace, so all four appear in the sample PDF.

## Still not TeX

- **Tags are not a column.** amsmath can align tags to the right edge of a
  wider block; here every tag sits at the content width.
- **No `\intertext` / `\shortintertext`**, no `\tag` inside `\substack`, no
  `alignat`/`flalign`.
- **The assembled brace is our own drawing.** Its proportions are fixed, not
  read from the font's MATH table; a face with a very different brace weight
  would still get our stroke.
- **`\text{}` handles `$…$` and `\$` only** — no nested `\text`, no macros, and
  the inner formula is laid out in the current style, not re-entering text
  style for nested `\text{}`.
- **The face switch is per character, not per run.** A `\text{}` run that
  starts Latin and ends in Han is measured and drawn correctly, but the
  kerning between the two faces comes from neither: each glyph advances by its
  own width.
