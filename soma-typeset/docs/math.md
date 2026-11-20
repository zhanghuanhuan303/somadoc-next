# Display math

`soma-math` turns a TeX subset into an integer box: positioned symbols, rules, a
width, an ascent and a depth. `soma-layout` places that box; renderers draw it.

Design record: ADR-0026. Known gaps: `docs/debt.md` D71.

## Pipeline

```text
MathBlock source
  └─ soma_math::parse      → Vec<Node>          (syntax)
  └─ soma_math::lay_out    → MathLayout          (integers, no font)
  └─ soma_layout::math     → MathBox             (adds baseline, centring)
  └─ soma_layout           → PlacedBlock.math    (one page box)
  └─ renderers             → <text>/<rect>, <span>/<div>
```

Inline math takes a parallel path (ADR-0027):

```text
MathInline source
  └─ soma_math::lay_out (text style)  → MathLayout
  └─ soma_layout::math::InlineMaths   → marker atom (U+E001 + index)
  └─ break_paragraph via ObjectMetrics → atom width/ascent/descent
  └─ PlacedBlock.inline_math          → {atom, source, math}
  └─ renderers                        → drawn at the atom origin
```

`soma_math` knows nothing about pages, styles, or fonts. `soma-layout` supplies
metrics through `MathMetrics`, implemented by `MathAdapter` over the same
`soma_core::Metrics` face the paragraph uses.

## Units and coordinates

- Everything is `i32` in 1/4096 em (`Width`).
- `MathItem.y` is measured from the **formula baseline**; negative is above.
- `MathItem.stretch` is permille of the natural advance (`1000` = as is); only
  the stretchy braces go above it today. `MathItem.color` is `None` for the
  surrounding text colour, or `0xRRGGBB`.
- `MathRule.y` is the top edge of the rule, also baseline-relative; a fraction
  bar is above the baseline, a radical bar covers the body.
- `MathBox.baseline` is the distance from the block top to the formula baseline.
  `MathBox.height` is `ascent + depth` (`MathLayout::total_height`).
- Placement: `x = (content_width - width) / 2`, `y = 0` inside the block. An
  equation number is appended to `items` with `x` already relative to the box
  origin, so the centring offset is subtracted from it.

## Syntax

| Form | Result |
|---|---|
| `x`, `中`, `\alpha` | one `MathItem`; a bare ASCII letter becomes `Node::Letter` and is set in the Mathematical Italic block when the face has it |
| `{...}` | group (inherits the enclosing size) |
| `x^2`, `x_1`, `x_i^2` | script to the right; superscript above, subscript below |
| `\frac{a}{b}` | numerator/denominator at `script_scale`, one `MathRule` at `axis_height` |
| `\sqrt{x}`, `\sqrt[3]{x}` | stretched `√` glyph plus a bar over the body; the degree is set at script size |
| `\sum_{i=1}^{n}`, `\prod`, `\lim`, `\sup`, `\inf` | big operator at `big_scale`; limits stack above and below |
| `\int_0^1`, `\oint` | big operator; limits stay to the right |
| `\log \ln \exp \sin \cos \tan \max \min` | upright name |
| `\text{...}` | raw text at the current size, no math spacing; `$...$` inside it is maths again (`\$` is a literal dollar) |
| `\,` `\;` `\!` `\quad` `\qquad` | fixed spaces (167/278/-167/1000/2000 permille) |
| `\left( ... \right)` | fences stretched to the body, centred on it; `()[]\{\}\|`, `\langle`, `\lfloor`, `\lceil` |
| `\left.`, `\right.` | null delimiter, draws nothing |
| `\begin{matrix}` / `pmatrix` / `bmatrix` / `vmatrix` | grid of cells; centred columns, fenced as named |
| `\begin{cases}` | one left brace, left-aligned columns |
| `\begin{aligned}` (`align`, `split`) | columns alternate right/left around `&`, no fences |
| `\begin{array}{lcr}` | like `matrix`, but each cell follows the column's `l`/`c`/`r` |
| `\substack{a\\b}` | lines stacked centred on each other, `frac_gap` apart |
| `\overbrace{x}`, `\underbrace{x}` | the font's brace glyph while the sideways stretch stays within 2000‰; wider bodies are assembled from rules (flat run, two hooks, a cusp) so the hooks never flatten |
| `\overbrace{x}^{n}` | the label is centred over the brace, not set after it |
| `\tag{A}`, `\tag*{A}` | this row's own label, in place of the automatic number (`\tag*` drops the parentheses); `$...$` inside the tag is maths |
| `\notag`, `\nonumber` | this row takes no number, and does not use one up |
| `\color{red} …`, `\color{#c00} …` | colours the rest of the group |

Also mapped: Greek letters (lower and upper), `\le \ge \ne \equiv \approx \sim
\propto`, arrows (`\to`, `\leftarrow`, `\Leftrightarrow`, …), set operators
(`\in \subset \cup \cap \emptyset`, …), `\partial \nabla \infty`, `\cdots
\ldots`, `\angle \perp \parallel`, `\cdot \times \div \pm \mp`.

Unsupported commands are a hard error: `MathError::UnsupportedCommand`. Layout
catches it and keeps the text placeholder, so a formula outside the subset is
visible in output instead of silently wrong. An unknown `\color` name is also
an error rather than a silent black.

## Numbering

`numbering.equations: "({n})"` in the template numbers display formulas whose
source is an `equation`, `align`, `gather` or `multline` environment. The `*`
variants and the inner `aligned`/`split` are never numbered. The counter runs
per document; one number is added per aligned row, right-aligned in the content
box, at that row's baseline. `MathLayout::rows` reports the row baselines and
what each row does about its number (`Auto`, `Suppressed`, `Tag { text, bare }`);
the labels themselves are appended by `soma-layout`, so `soma-math` stays
unaware of page geometry.

Two rules keep numbering predictable:

- **Only the outermost block owns rows.** An `aligned` inside an `equation`, or
a `cases` inside an `aligned`, lays out normally and contributes no rows, so a
formula with a nested block still gets exactly one number per real row.
- **A tag costs no number.** `\tag` and `\notag` leave the counter alone, so
adding a tag to one row does not renumber the rest.

A tag is laid out as `\text{...}`, which is why `\tag{A}` prints upright and
`\tag{$\alpha$}` prints `(α)` rather than the macro name.

## Inline math

`$...$` / `\(...\)` sources become **object atoms** (`AtomKind::InlineObject`):

- laid out in **text style** (`MathStyle { display: false }`), so `\sum`/`\prod`
  limits follow the operator instead of stacking;
- measured by their box: the kernel calls
  `Metrics::object_advance` / `Metrics::object_metrics` for object atoms, and
  `soma-layout::math::ObjectMetrics` answers from the laid-out formula;
- identifed by a marker (`U+E001` + ordinal) so a body-text atom can never be
  confused with a formula;
- recorded as `PlacedBlock.inline_math: [{atom, source, math}]`, where `atom`
  indexes the block's text stream;
- drawn by both renderers at
  `x = atom.x`, `y = line top + line ascent` (baseline), never as text.

A formula that does not parse keeps the old placeholder behaviour: the source
is one text atom and `inline_math` stays empty. Inline formulas never split
across lines, participate in justification only as fixed-width atoms, and raise
the line box through their ascent/depth.

Text style has its own constants: fractions shrink to `text_frac_scale` with a
tighter `text_frac_gap`, and big operators use `text_big_scale`. Remaining
inline limits (D72): no italic math font and no per-script kerning.

## Style constants (`MathStyle`)

| Field | Default | Meaning |
|---|---|---|
| `script_scale` | 700 | superscript/subscript/limit size |
| `scriptscript_scale` | 500 | reserved for nested scripts |
| `sup_shift` | 400 | minimum superscript raise |
| `sub_shift` | 200 | minimum subscript drop |
| `axis_height` | 250 | fraction bar position above the baseline |
| `rule_thickness` | 40 | fraction/radical bar thickness |
| `frac_gap` | 120 | gap between bar and numerator/denominator; also the limit gap |
| `bin_gap` | 220 | space around `+ - × ·` |
| `rel_gap` | 280 | space around `= < ≥` |
| `punct_gap` | 170 | reserved for `, ;` |
| `big_scale` | 1400 | big operator size, display style |
| `text_frac_scale` | 700 | numerator/denominator size, text style |
| `text_frac_gap` | 80 | fraction bar gap, text style |
| `text_big_scale` | 1100 | big operator size, text style |
| `display` | `true` | `false` = text style: limits to the right, smaller operators |

Display fractions use full-size parts (`1000`); text style uses
`text_frac_scale`. All values are permille of the current size.
`soma-layout::math::style_for`
takes everything from `MathStyle::default()` and replaces `size` with the
resolved `font_size`, so headings and 8 pt footnotes typeset at their own size
with no extra configuration.

## Placeholder fallback

`math::display` / `math::display_inline` return `None` for a parse error.
Display: `Builder::math_item` then returns `None` and `flow_into` calls
`object_item(BoxKind::Math, source, style)` — the source is laid out as one
text line. Inline: the atom text stays the source and is measured as text.
Either way the page never fails because of a formula.

## Metric source

Formula symbols come from a dedicated **math face** (family `2`) in the metrics
table, generated from Noto Sans Math. `Metrics::math_advance` and
`Metrics::math_metrics` are the seam; `TableMetrics`/`StyledMetrics` answer from
that face at the style's font size, so `∑`, `∫`, `√`, Greek and relations get
their real advances and extents. Adapters without a math face keep the body
font (the trait defaults), and their output is byte-identical to before.

A character the math face does not carry — Han inside `\text{...}` is the common
case — is measured and drawn with the style's own face, then the rescue face.
`StyledMetrics::math_advance` walks the characters and sums the advance of the
face that will draw each one; `soma-render-pdf` splits an item into runs the
same way, so width and paint agree.

## Pagination

A display formula is one indivisible flow item: it has glue before/after from
the style, `keep_with_next` and `page_break_before` are honoured, and it never
splits. If it does not fit on an empty page the page is marked `overflow` like
any other tall box. An inline formula is an indivisible *atom*: it moves to the
next line with the rest of the line and never splits mid-formula.

## Ink, not the em box

A glyph's font-wide ascent is 1.05 em; a line of lowercase letters tops out
near 0.7 em. Anything that has to *touch* the text — a brace, a stacked limit —
must be placed against the ink, or it floats. `Context::measure_ink` measures a
node with a cap-height estimate (`min(ascent, 0.72 em)`) and a short descender
(`min(descent, 0.22 em)`) instead of the font extents.

The narrow brace path needs one more fact: the glyph's ink is a narrow band far
from its own baseline. `brace_ink` holds the measured band (`U+23DE`
0.656…0.893 em above the baseline, `U+23DF` 0.075…0.310 em below it), and both
the placement and the box measurement use it. The table is public so a renderer
— or a test — can ask where the ink actually is.

The wide brace path does not need the table at all: it draws the brace itself,
so its extents are the rules' extents. `BRACE_GLYPH_STRETCH` (2000‰) is where the
layout stops stretching the glyph and starts assembling: a flat run, two
quarter-circle hooks and a straight-sided cusp, one rule per slice, each slice
covering the level range of both its ends so the stroke stays continuous where
the curve is steep.

## Tests

| File | What it locks |
|---|---|
| `soma-math/tests/math.rs` | scripts, fractions, radicals, big operators, fences, matrices, aligned, cases, names, spacing, errors, italic variables, `array` formats, `\substack`, brace ink and labels, assembled braces, `\color`, text-mode maths, tags |
| `soma-layout/tests/e11_math.rs` | centring, baseline inside the box, line flow, canonical JSON |
| `soma-layout/tests/e12_inline_math.rs` | marker atoms, line-box growth, text-style limits, placeholder, canonical `inlineMath` |
| `soma-render-svg/tests/math.rs` | exact `<text>`/`<rect>` coordinates |
| `soma-render-svg/tests/inline_math.rs` | inline items on the line baseline, marker never drawn |
| `soma-render-html/tests/math.rs` | span/div preview, placeholder fallback |
| `soma-render-html/tests/inline_math.rs` | inline preview positions, marker never drawn |
| `soma-layout/tests/a6_blocks.rs` | display box vs placeholder |
| `soma-core/tests/k7_object_metrics.rs` | object hooks vs text paths, default-hook equivalence |
