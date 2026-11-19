# ADR-0027: Inline math as object atoms

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0007 (frozen inline kernel), ADR-0026 (display math),
  `docs/debt.md` D54
- Touches: `soma-core` `Metrics` seam (two defaulted methods),
  `soma-layout` (`InlineMathBox`), page schema `soma.pages/6`

## Context

ADR-0026 typeset display formulas. `MathInline` was still drawn as its source
text: the atom was measured with the font path, so `x^2` occupied the width of
the five characters `x`, `^`, `2`, spaces included, and the line box never grew
for a fraction. A document that mixes prose and formulas — the common case for
Chinese technical writing — therefore typeset its inline math as TeX source.

## Decision

### The kernel needs a kind-aware metrics hook

Line breaking measures atoms through `Metrics::advance`, and the line box
measures them through `Metrics::glyph_metrics`. Both take `(text, style)`, so an
adapter cannot tell a formula from text that happens to look like one.

The kernel now dispatches on `AtomKind`:

- `AtomKind::InlineObject` → `Metrics::object_advance` / `Metrics::object_metrics`;
- everything else → the existing text paths.

Both new methods are **defaulted** to the text paths, so:

- no existing adapter changes;
- a document without objects produces byte-identical output;
- the DP, the cost model, the fallback ladder, the tie-break and the trim logic
  are untouched.

### Markers, not source text

`soma-layout` lays a formula out before line breaking (`math::display_inline`,
text style: big-operator limits stay to the right) and replaces the atom text
with a private-use marker `U+E001` + ordinal. Markers matter: keying the metrics
lookup on the source would let a body-text atom `x` inherit the geometry of an
inline formula `$x$` in the same paragraph.

`InlineMaths` keeps `(marker, source, layout)`; `ObjectMetrics` — a `Metrics`
proxy built per paragraph — answers the two object hooks for markers and
forwards every other method to the wrapped source.

### Result shape

`PlacedBlock.inline_math: Vec<InlineMathBox { atom, source, math }>` where
`math` is the same `MathBox` type display math uses. `atom` indexes the block's
text stream, so renderers match it against `line.boxes[].atom`. The source is
kept for diagnostics and text extraction, not for drawing.

Page schema becomes `soma.pages/6`: every block gains
`"inlineMath": [{"atom":N,"source":"…","math":{…}}]` (empty array when there is
no inline math).

### Rendering

Inline items are baseline-relative, and the line baseline is already known:

- SVG: `x = margin_left + block.x + atom.x`, `y = margin_top + block.y +
  placed.y + line.ascent + item.y` — exact.
- HTML: the same baseline, with the display-math `7/8 em` preview approximation.

Both renderers skip the marker atom: an atom found in `inline_math` is never
emitted as text.

### Failure path

A source that does not parse keeps the pre-ADR behaviour: the atom text is the
source and it is measured as text. A malformed formula never fails layout, and
`inline_math` stays empty.

## Consequences

- Inline formulas now break lines by their box width and raise the line box by
  their ascent/depth, which is what `Line.ascent`/`Line.descent` already
  supported for large inline glyphs.
- No `MathInline` API, DocIR node, template key or engine knob changed.
- `soma-core` changed by two defaulted trait methods and two dispatch sites.
  Every existing test keeps its numbers; `k7_object_metrics.rs` locks the new
  behaviour and the default-hook equivalence.
- New debt D72: text-style constants are not yet differentiated from display
  ({`\\frac` parts keep `script_scale`, `big_scale` does not shrink), no italic
  math font, no delimiter sizing.

## Alternatives rejected

- **Key object metrics on the source text.** A one-letter formula `$x$` would
  collide with the letter `x` in the same paragraph.
- **Measure inline math as text (status quo).** The line box ignores formula
  height and the width is unrelated to the drawn glyphs.
- **Add a `math: Option<…>` field to `Atom`.** The kernel would carry a
  presentation payload and every constructor would change; the metrics seam
  already exists for exactly this question, and object atoms already exist.
- **Lay inline math out inside the breaker.** The kernel would depend on
  `soma-math`; the frozen kernel must stay unaware of math.
