# Diagrams: Mermaid and Graphviz

A diagram is a semantic `Block::Diagram` in DocIR (`soma.doc/3`). `soma-diagram`
compiles it to the same integer vector box as TikZ (`paths` + text items +
rules), so PDF, SVG and HTML need no diagram-specific renderer. Design record:
ADR-0102.

## Source

A diagram is recognised from a fenced code block, an HTML `pre`/`div` class, or
a LaTeX environment:

| Source | Result |
|---|---|
| ` ```mermaid ` / ` ```mmd ` / ` ```flowchart ` | Mermaid flowchart |
| ` ```dot ` / ` ```graphviz ` | Graphviz DOT |
| An untagged fence whose first meaningful line is `flowchart <dir>` / `graph <dir>` | Mermaid |
| An untagged fence whose first meaningful line is `digraph` / `strict digraph` / `graph` | DOT |
| ` ```text ` / ` ```plain ` / ` ```code ` / ` ```mermaid-source ` / ` ```dot-source ` | Always a code block |
| `<pre class="mermaid">`, `<div class="mermaid">` | Mermaid |
| `\begin{mermaid}…\end{mermaid}` | Mermaid |
| `\begin{dot}…\end{dot}` / `\begin{graphviz}…\end{graphviz}` | DOT |

Plain paragraphs are never sniffed. A diagram that fails to parse or lay out is
drawn as its original source in a code panel; it never disappears.

## Mermaid subset

- header: `flowchart TD|TB|BT|LR|RL`, legacy `graph …`;
- nodes: `A[text]`, `A(text)`, `A([text])`, `A[[text]]`, `A[(text)]`,
  `A((text))`, `A{text}`, `A{{text}}`, plus the parallelogram, trapezoid and
  asymmetric forms;
- edges: `-->`, `---`, `-.->`, `==>`, `<-->`, `--o`, `--x`, `~~~`, longer
  variants such as `--->`, labels with `-->|text|` or `-- text -->`;
- chains `A --> B --> C`, fan-out `A & B --> C`, subgraphs
  (`subgraph title … end`), `classDef`/`class`/`style`/`linkStyle`;
- `%%` comments, `%%{init}%%`, `click`, `accTitle`, `accDescr` are inert.

Shapes lower to PGF paths: rectangle, rounded rectangle, stadium, subroutine,
cylinder, circle, double circle, diamond, hexagon, parallelogram, trapezoid,
asymmetric, ellipse.

## Graphviz subset

- `digraph`/`graph`, optional `strict`, optional graph id;
- `rankdir=TB|LR|RL|BT`, `label=…` on a cluster;
- `node [shape=…, style=…, label=…]`, `edge [style=…, arrowhead=…]`;
- edge chains `a -> b -> c`, optional `[attrs]`, `constraint=false`;
- `subgraph cluster_… { label="…"; … }`;
- shapes: box/rect/rectangle/square/note/component/folder/tab/box3d, ellipse,
  oval, egg, circle, point, doublecircle, diamond, hexagon, cylinder,
  parallelogram, trapezium, plaintext/none. Unsupported shapes are an error.
- labels may be quoted; `\n`, `\l`, `\r` and `<br/>` become line breaks.

## Layout and limits

- deterministic layered layout: cycle removal, longest-path ranks, dummy nodes,
  eight barycenter sweeps, declaration-order tie-breaks;
- orthogonal routing; back edges use an outer channel; self-loops are routed
  after layering; parallel edges share ports;
- CJK labels use the document's own metrics and punctuation rules; `<br/>`
  forces a line; a label may also wrap after `/` or `-` (`CoreAudio/AAudio/…`);
- a wide diagram is scaled down (never up); below 60% of the measure, or taller
  than the page content box, it falls back to source;
- the label cap is fitted: the diagram is laid out with 16, 12, 10 and 8 em
  label widths and the candidate with the largest final scale wins, so a wide
  flowchart wraps its node text instead of shrinking the labels to 60%;
- hard limits: 2000 nodes, 8000 edges, 200 subgraphs, 256 KiB source.

## TikZ export

```sh
soma diagram --input chart.mmd                 # TikZ on stdout
soma diagram --input chart.mmd --out chart.tikz
soma diagram --input chart.dot --template book.yaml --metrics theme.smt --out chart.tikz
```

The export uses the computed coordinates in explicit `em` units and only
`\path`/`\node`; it needs no TikZ libraries. `soma-tikz` parses it back to the
same geometry (the round-trip test in `crates/soma-diagram/tests/mermaid.rs`).
In Soma Studio, `⤴ TikZ` calls `POST /api/tikz` and downloads the first diagram
in the active tab. The Studio export uses that diagram's computed style and
keeps its natural size: page-width scaling and page-height fallback apply only
to the proof, not to the exported source (ADR-0103).

## Template

`element: diagram` styles the display block. The Plex default sets body-sized
labels and `0.8em` before/after. Node/edge colours come from the built-in print
theme (white fill, black stroke, black text) and Mermaid `classDef`/`style`
overrides; no web pastel theme is applied.

## Not in this version

Sequence, class, state, ER, gantt, pie, mindmap, timeline, journey, C4, icons,
HTML labels, external images, links, animations. Subgraphs are frames around
their members; they do not constrain rank order yet.
