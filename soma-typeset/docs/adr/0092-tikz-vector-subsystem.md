# ADR-0092: TikZ is a vector subsystem, not a text fallback

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0026 (display math), ADR-0070 (Math v2), ADR-0075 (math
  variants), ADR-0090 (code panel), ADR-0091 (syntax lexer)
- Touches: new crate `soma-tikz`, `soma-layout` (`MathBox.paths`,
  `math_item`), `soma-latex` (`tikzpicture`), `soma-render-pdf`,
  `soma-render-html`, `soma-render-svg`, `soma-core` (path data only),
  `docs/capabilities.md`, `docs/tikz.md`, `examples/tikz/`

## Context

A `tikzpicture` is currently parsed as unknown LaTeX: its `\draw` commands
become text, which is the worst possible result — it looks like a broken
document and silently loses the figure. The math subsystem already solved the
same class of problem: parse a TeX-shaped source into integer geometry, draw it
with the existing font/rule backends, and fail loudly on constructs outside the
subset.

TikZ is larger than TeX math, so the subsystem must state its boundary. The
goal of this ADR is **PGF's coordinate and path semantics for the core drawing
language**, not the TikZ libraries: no `plot`, `decorations`, `graphs`,
`matrix`, `pic`, `shadings`, `patterns`, `clipping`, 3D or externalization.
An unsupported key or command is an error, never a silent approximation.

## Decision

### One vector box

The layout already has `MathBox` (positioned glyphs, rules, baseline). TikZ
needs the same coordinate frame plus stroked/filled paths. `MathBox` gains
`paths: Vec<soma_tikz::Path>`; ordinary formulas leave it empty, so their
canonical bytes and hashes do not change.

A path is an integer, baseline-relative vector:

```text
start (point), segments [line | cubic | close],
fill: Option<u32>, stroke: Option<Stroke>
Stroke { color, width, dash: [on, off, …] }
```

Coordinates are `1/4096 em` (the engine's `Width`) at the boundary; inside
`soma-tikz` they are `1/65536 em` so a 0.1 cm coordinate, a 0.4 pt line and a
rotation do not lose the last bit. The conversion is integer truncation, the
same rule as every other layer.

`Point.y` is baseline-relative with negative above, matching `MathItem.y`.
A TikZ picture's baseline is its bounding-box bottom, which is PGF's default
for a picture used in a paragraph. The picture is centred by the existing math
block placement; a template controls the surrounding skips through
`mathBlock`.

### Geometry

- Default coordinate unit: `1cm`. `1cm = 3600/127 pt`, `1pt = 1/72 in`,
  matching the engine's exact unit table; `em` is the document body em.
- Default line width: `0.4pt`; `thin/very thin/semithick/thick/very thick/
  ultra thick` use PGF's 0.2/0.8/… values.
- Default dash arrays are PGF's named patterns (`dashed` = 3pt on/3pt off,
  `dotted` = line width on/2pt off, …).
- Node defaults: `inner sep = 0.3333em`, `outer sep = 0.5 line width`,
  `anchor = center`, rectangle shape; text is centred by the text box's
  ascent/depth, not by the font's em box.
- A node's anchors (`north`, `south east`, …), `above/below/left/right`,
  `minimum width/height`, `draw/fill`, `circle`, `rounded corners` and
  `inner/outer sep` follow the PGF node model.
- Coordinates: Cartesian and polar, `+`/`++`, named coordinates and node
  anchors. Trigonometry is integer Taylor series; no floating point.
- Paths: `--`, `-- cycle`, `rectangle`, `circle`, `ellipse`, `arc`, `grid`;
  `\draw`, `\fill`, `\filldraw`, `\path`; `node`/`coordinate` on a path.
- `\foreach \x in {a,b,…}{…}` is expanded before parsing for the common
  coordinate-labeling idiom.

### LaTeX frontend

`\begin{tikzpicture}[…] … \end{tikzpicture}` becomes a `MathBlock` whose source
keeps the environment wrapper. This deliberately avoids a DocIR schema bump:
the block is still "display object", the existing `mathBlock` selector and
atomic pagination apply, and `soma-math` is not asked to parse TikZ. The layout
recognises the `tikzpicture` prefix before calling the math parser.

Node text is laid out through the math subsystem as `\text{…}` at the
**document** font size (not `mathBlock`'s 12pt), because TikZ nodes inherit the
surrounding LaTeX font, not the display-formula size. `$…$` inside a node is
therefore real math, and CJK falls back through the normal metrics path.

### Renderers

The three backends draw `MathBox.paths` before the text:

- PDF: `m/l/c/h` path operators, `f`/`S`/`B`, line width, dash array, stroke
  colour. Paths are behind the node labels.
- SVG: `<path d="…">`.
- HTML: one absolutely positioned inline `<svg>` per vector box, behind the
  existing absolutely positioned math spans.

Arrow tips (`->`, `<-`, `<->`) are emitted as small stroked barbs from the path
endpoint. They are a geometric approximation of PGF's `to` tip, not a glyph
copy; the ADR calls this out so a future arrow library can replace them
without changing the box tree.

## Consequences

- A malformed or unsupported TikZ block follows the math failure path and stays
  visible as its source text; it never disappears.
- Normal formulas keep `paths = []`; their canonical JSON and hashes are
  unchanged. A TikZ block adds an optional `"paths"` member to its math box.
- Text inside TikZ reuses `soma-math`'s `\text` and `$…$` handling; there is no
  second text stack.
- `soma-tikz` is `no_std + alloc`, integer-only, dependency-free apart from
  `soma-core` and `soma-math`; it is checked on wasm like the rest of the
  core.
- The subset is intentionally smaller than TikZ. A document using `plot`,
  `decorate`, `matrix`, `pic`, `clip`, patterns or a positioning-library key
  gets a clear layout fallback, not a wrong picture.

## Alternatives

- **Treat TikZ as an image.** It loses scalability, text colour, font matching
  and reproducibility; rejected.
- **Add a `Block::Tikz` DocIR node.** Clean in the long run, but a schema bump
  and every frontend match for one adapter; the raw/math object route already
  has the right pagination and style semantics. Revisit if TikZ gains
  document-level semantics (captions, float placement) beyond a display
  object.
- **Use a general SVG/TikZ crate.** Dependency and determinism cost; the core
  language is small enough to own.
- **Approximate unknown keys.** Silent wrong diagrams are worse than a visible
  fallback; rejected.

## Alignment

- `crates/soma-tikz/` — parser, fixed-point geometry, node anchors, paths.
- `crates/soma-layout/src/lib.rs` — `MathBox.paths`, `math_item` dispatch.
- `crates/soma-latex/src/lib.rs` — `tikzpicture` environment.
- `crates/soma-render-*/src/lib.rs` — path painting.
- `crates/soma-layout/src/canonical.rs` — optional paths in the math payload.
- `examples/tikz/` — LaTeX reference and soma sample.
- `docs/tikz.md`, `docs/capabilities.md`, `docs/adr/README.md`.
