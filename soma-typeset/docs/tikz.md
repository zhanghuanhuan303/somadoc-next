# TikZ vector drawings

`soma-tikz` turns the core PGF drawing language inside a `tikzpicture` into an
integer vector box: paths (line, cubic, close), fill/stroke/line width/dash, and
node labels measured through the math subsystem. `soma-layout` places that box
as one display object; all three renderers draw it.

Design record: ADR-0092. Known gaps: `docs/debt.md` D99.

## Pipeline

```text
tikzpicture source
  └─ soma-tikz::layout     → TikzLayout       (paths + positioned text items)
  └─ soma-layout::math     → MathBox.paths    (display object, centred)
  └─ soma-layout           → PlacedBlock.math (one page box, atomic)
  └─ renderers             → m/l/c/h + S/f/B, <path>, inline <svg>
```

The LaTeX frontend keeps the `\begin{tikzpicture}…\end{tikzpicture}` wrapper in
a `MathBlock`; `soma-layout` recognises the prefix and calls `soma-tikz` before
`soma-math`. Nothing else in DocIR changes: a picture is a display object, so
existing `mathBlock` style rules, `keep_with_next`, page breaks and fallback
behaviour apply.

## Units and coordinates

- Everything is integer. `soma-tikz` works in `1/65536 em` internally and
  converts to the engine's `1/4096 em` (`Width`) at the boundary; the conversion
  truncates toward zero, like every other layer.
- `Point.y` is baseline-relative, negative above, matching `MathItem.y`. The
  picture's baseline is the bottom of its bounding box (PGF's default for an
  inline picture).
- Default coordinate unit is `1cm`; lengths in options default to `pt`, except
  `inner sep`/`outer sep` (`em`) and grid steps (`cm`). `1cm = 3600/127 pt`,
  `1em = 65536` internal units.
- `scale`, `xshift`, `yshift` on the picture or on a path transform coordinates
  and grid steps.
- Trigonometry (`arc`, polar coordinates) uses integer Taylor series and
  `isqrt`; there is no floating point anywhere in the pipeline.

## Syntax

| Form | Result |
|---|---|
| `\draw`, `\fill`, `\filldraw`, `\path` | start a path; `\draw` strokes, `\fill` fills, `\filldraw` does both, `\path` paints only what its options ask for |
| `(x,y)`, `(angle:radius)` | Cartesian or polar coordinate |
| `(A)`, `(A.east)` | named coordinate / node anchor |
| `+`, `++` | relative to the previous point (`+` does not update it) |
| `--`, `-- cycle` | straight segment, closed subpath |
| `rectangle (x,y)` | axis-aligned rectangle |
| `circle (r)`, `circle [radius=r]` | circle as four cubic quarters |
| `ellipse [x radius=…, y radius=…]` | ellipse as four cubic quarters |
| `arc (start:end:radius)`, `arc [start angle=…, end angle=…, radius=…]` | circular arc as cubic quarters, ≤ 90° each |
| `grid (x,y)` | lattice lines between the current point and the corner |
| `-|`, `\|-` | orthogonal two-segment connection |
| `node [opts] at (x,y) {text}`, `node [opts] {text}` | box with optional fill/stroke and a centred label |
| `coordinate (A) at (x,y)` | named point, draws nothing |
| `\foreach \x in {a,b,…}{…}` | expanded before parsing (common coordinate-labelling idiom) |

Path options: `color` / bare colour name (`red`, `red!20!white`, `#c00`),
`draw`, `fill`, `text`, `line width`, `thin`/`very thin`/`semithick`/`thick`/
`very thick`/`ultra thick`, `dashed`/`dotted`/`densely dashed`/`loosely dashed`/
`densely dotted`/`loosely dotted`, `dash pattern`, `->`/`<-`/`<->`, `scale`,
`xshift`, `yshift`, `step`/`xstep`/`ystep`.

Node options: `draw`, `fill`, `text`, colours, line width, `inner sep`,
`outer sep`, `minimum width`/`minimum height`/`minimum size`, `anchor`,
`above`/`below`/`left`/`right` (and the four diagonals), `circle`,
`rectangle`, `rounded corners`, `scale`.

Defaults follow PGF: line width `0.4pt`, `inner sep = .3333em`,
`outer sep = .5 × line width`, `dashed = 3pt on/3pt off`, grid step `1cm`,
arrow tips as stroked barbs.

## Grid

`\draw (0,0) grid (3,2);` draws vertical lines at every x multiple and
horizontal lines at every y multiple of the step, **including both border
lines**. The far edge is always drawn even when the span is not an exact
multiple. `\draw[step=0.5]`, `step=(0.5,1)`, `xstep=…`, `ystep=…` change the
lattice. `draw` strokes every lattice line; `fill` has no area, exactly as in
PGF, where a grid is a set of line subpaths. A non-positive step and a lattice
over 10,000 lines are hard errors.

## Node labels

Node text is measured through the math subsystem as `\text{…}` at the document
font size (TikZ nodes inherit the surrounding font, not the display-formula
size). `$…$` inside a node is real maths, and CJK falls back through the normal
metrics path. A node is one unbreakable unit; multi-line nodes (`\\`) are a hard
error. The label is centred by its ascent/depth, not by the font's em box.

## Rendering

- PDF: `m/l/c/h` operators, `S`, `f`, `B`, `w` line width, dash array,
  stroke/fill colour, with node labels drawn after the paths.
- SVG: `<path d="…" fill="…" stroke="…">` in the same coordinate frame.
- HTML: one absolutely positioned inline `<svg>` per vector box, behind the
  existing positioned math spans.

Arrow tips are geometric barbs (two stroked segments) at the path endpoint, an
approximation of PGF's `to` tip rather than a glyph copy.

## Failure semantics

The parser is strict: an unsupported command, environment, key, anchor, colour
or node construct is a `TikzError`, never a silent approximation. The layout
turns that into the same placeholder as a failed formula — the original source
stays visible as text, so a figure is never silently lost or drawn wrong.

## Known differences from PGF

The subset is the core drawing language, not the libraries:

- no `plot`, `decorations`, `graphs`, `matrix`, `pic`, `shadings`, `patterns`,
  `clip`, 3D, externalization or the `positioning` library;
- arrow tips are barbs, not the full `arrows.meta` vocabulary;
- rotation is not exposed as an option yet;
- `arc` bounding boxes use the full circle, so they can be conservative;
- `ellipse` and `circle` are cubic approximations (four quarters), as in most
  vector backends;
- node text cannot contain `\\`.

These are documented rather than emulated silently; the ADR calls out the
boundary so a future library can be added without changing the box tree.

## Tests

| File | What it locks |
|---|---|
| `soma-tikz/tests/tikz.rs` | centimetre unit, shift/scale, circle/arc cubics, named coordinates and anchors, `\foreach` and a tail statement, grid lattice and step, a filled grid has no area, node measurement, recursive node/arrow paths |
| `soma-layout/tests/e27_tikz.rs` | one centred vector box, node labels as items, canonical `paths` only when present, source fallback, atomic flow |
| `soma-render-svg/tests/gate_r.rs` | vector path output |
| `soma-render-html/tests/*` | inline `<svg>` per vector box |
| `soma-render-pdf/src/lib.rs` (unit) | a path fill does not leak into the next text run (the painter's fill cache) |

## Example

`examples/tikz/figure.md` is the soma input; `examples/tikz/figure.tex` is the
same picture for `pdflatex`, used as the visual reference. `scripts/plex.sh`
renders the sample to `examples/templates/out/tikz-sample.pdf`.
