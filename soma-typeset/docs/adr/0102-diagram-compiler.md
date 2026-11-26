# ADR-0102: Diagrams are semantic source, compiled to the TikZ vector box

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0092 (TikZ vector subsystem), ADR-0088 (Plex default),
  ADR-0090 (code panel), ADR-0101 (Studio hyphenation)
- Touches: `soma-doc` (`Block::Diagram`, `soma.doc/3`), `soma-diagram` (new),
  `soma-md`, `soma-html`, `soma-latex`, `soma-layout`, `soma-template`
  (`element: diagram`), `soma-cli` (`soma diagram`), `examples/templates/plex.yaml`

## Context

Models routinely emit flowcharts as fenced Mermaid (`flowchart TD …`) and, less
often, Graphviz DOT. The engine used to render those fences as code panels, so
the diagram was invisible. TikZ already had a complete vector path
(`soma-tikz` → `MathBox.paths` → PDF/SVG/HTML), but it only accepted a
hand-written `tikzpicture`; it has no graph parser and deliberately no automatic
layout.

A second problem was semantic. A diagram is content, not code. Keeping it in
`CodeBlock` would make templates, accessibility, text extraction and future
frontends guess from a language tag. The user explicitly chose a DocIR design,
untagged sniffing, a black-and-white print look, and a Tavily-style
“scale, then fall back below 60%” width policy.

## Decision

### DocIR

- Add `Block::Diagram(DiagramBlock { dialect, source })` with
  `DiagramDialect::{Mermaid, Graphviz}`. Bump the canonical schema to
  `soma.doc/3` and `docIrVersion = 3`. The block owns the dialect; layout never
  re-guesses it.
- Detection lives in `DiagramDialect::detect`:
  - explicit `mermaid`/`mmd`/`flowchart` and `dot`/`graphviz` fences become
    diagrams;
  - `text`, `plain`, `code`, `none`, `mermaid-source`, `dot-source` force code;
  - an untagged fence is sniffed only when its first meaningful line is
    `flowchart`/`graph <dir>` or `digraph`/`strict digraph`/`graph`;
  - plain paragraphs are never sniffed.
- LaTeX gains `mermaid`, `dot`, and `graphviz` environments; HTML gains
  `<pre class="mermaid|dot|graphviz">`, `<div class="mermaid|dot|graphviz">`,
  and `language-mermaid`/`language-dot` code classes.
- `Block::Diagram` has no template-owned geometry. Its source is validated
  non-empty and capped at 256 KiB.

### The compiler

New `soma-diagram` crate (`no_std + alloc`, integer-only, no third-party
dependencies):

- **Frontends** parse Mermaid flowcharts and a Graphviz DOT subset into one
  `Diagram` IR (nodes, edges, subgraphs, styles).
- **Layout** performs deterministic Sugiyama-style layering: DFS cycle removal,
  longest-path ranks, dummy nodes for long edges, eight barycenter sweeps with
  declaration-order tie-breaks, rank/node spacing, then orthogonal routing.
  Self-loops route after layering; back edges use an outer channel; parallel
  edges share ports.
- **Shapes** lower to `soma_tikz::{Path, Segment, Stroke}`: rectangle, rounded
  rectangle, stadium, subroutine, cylinder, circle, double circle, diamond,
  hexagon, parallelogram, trapezoid, asymmetric and ellipse. Every edge is a
  line path with a filled arrow, open tip, circle, cross or none.
- **Labels** are measured and wrapped by a caller-supplied `TextEngine`.
  `soma-layout::DiagramText` implements it with the document metrics and CJK
  punctuation rules; the diagram crate never opens a font. `<br/>` forces a
  line, long Latin words stay whole, CJK characters may break individually, and
  closing punctuation is not left at a line start.
- Output is `soma_tikz::TikzLayout` (`paths` + `MathItem` + `MathRule`), so the
  PDF, SVG and HTML renderers need **no change**. The painter order is group
  frames → edges → node shapes → arrow tips; text is drawn last by the existing
  renderers.

### Placement, fallback, and limits

- A diagram is one atomic display block, centered in the measure. It uses the
  `diagram` template element and the Plex default `0.8em`/`0.8em` spacing.
- Wide diagrams scale down uniformly (never up). Below 60% or taller than the
  page content box, the block falls back to its original source in a code
  panel. A parse or layout error always falls back, never draws a partial
  picture.
- Limits: 2000 nodes, 8000 edges, 200 groups, 256 KiB source. The parser is
  strict: unsupported shapes and syntax are errors; web-only `%%{init}%%`,
  `click` and `accTitle` are inert.
- No HTML labels, external images, links or scripts. `click` never becomes an
  `<a>`; the renderer invariant is unchanged.

### TikZ correspondence

A diagram and a `tikzpicture` meet at the same `TikzLayout`. To make the
correspondence inspectable, `soma_diagram::to_tikz` emits the computed geometry
as plain `\path`/`\node` commands in explicit `em` units, using only the core
drawing language (no `shapes` or `arrows` libraries). `soma-tikz` learned
`anchor=base` and accepts whitespace before `cycle` so the exported source
round-trips. A test compiles Mermaid → geometry A, exports TikZ, parses it back
to geometry B, and asserts the same path count, text count and non-empty
geometry.

`soma diagram --input chart.mmd [--from mermaid|dot] [--out chart.tikz]` exposes
the export. `soma render` renders diagrams inside documents with no flag; the
Studio gets them through the same `soma-studio-core` build.

## Consequences

- `soma.doc/3` is a schema bump: the A1 golden and the schema assertions change.
  The Plex template gains an `element: diagram` rule, so its canonical hash and
  length change; the D31 reader and A2 reference-hash tests are updated.
- Existing documents without diagram fences keep byte-identical DocIR and page
  output. The corpus baseline is unchanged.
- Diagrams add optional `paths` to the existing math box; formulas and TikZ keep
  their canonical bytes.
- Unsupported Mermaid/DOT constructs remain visible as source code. That is the
  documented failure mode.
- The layout does not yet constrain subgraph membership to ranks; a cluster frame
  can enclose interleaved nodes. Sequence/class/state/ER diagrams are out of
  scope because they need a different layout model.

## Alternatives

- **Run Mermaid JS in the Studio.** Rejected: no PDF backend, no offline
  determinism, and a second renderer.
- **Mermaid → TikZ text → existing parser.** Rejected as the main path: double
  parsing, label escaping, and no structure. Kept only as an export format.
- **Keep diagrams as `CodeBlock` and detect by language in layout.** Rejected
  now that the semantic IR is being designed: templates and future frontends
  should see a diagram, not a code fence.
- **Emit raster images.** Rejected: violates the vector, deterministic,
  print-first goal.
