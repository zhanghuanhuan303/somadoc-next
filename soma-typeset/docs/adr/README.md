# Architecture Decision Records

Status: **frozen on 2026-11-20** with `docs/architecture-v2.md`.
ADR-0010 amends §15.1 (one extra crate) and fixes shapes that
architecture v2 left open; ADR-0012…0031 extend the implemented system
(table metrics, line-level pagination, typography, page masters, engine
configuration, table continuation, table rules and context selectors,
frontends, display and inline math, contents pages, the CLI, math fences and
matrices, alignment, generated covers)
without changing the layer contract.

These ADRs record the decisions that shape the next phase. They are aligned
with the already-implemented code; they do not change the frozen inline
kernel's DP, cost model, or tie-break (ADR-0027 adds defaulted metrics hooks
only).

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-two-layer-ir.md) | Two-layer IR: DocIR + Layout Box Tree | Accepted |
| [0002](0002-clean-room.md) | Clean-room; no prior Typst project | Accepted |
| [0003](0003-pure-yaml-template.md) | Pure YAML template; no CSS engine | Accepted |
| [0004](0004-syntax-frontends.md) | Frontend trait; semantic whitelist | Accepted |
| [0005](0005-layout-and-pagination.md) | Box tree; vertical K-P + greedy fallback | Accepted |
| [0006](0006-determinism-and-versioning.md) | Integer units, canonical JSON, hash tuple | Accepted |
| [0007](0007-freeze-inline-kernel.md) | Freeze the existing inline K-P kernel | Accepted |
| [0008](0008-docir-migration.md) | New `soma-doc`; retire legacy semantic module | Accepted |
| [0009](0009-semantic-presentation-boundary.md) | DocIR semantics; template owns presentation | Accepted |
| [0010](0010-borrow-first-and-parallel.md) | Borrow-first layer boundaries; optional parallelism outside the core | Accepted |
| [0011](0011-inline-span-compiler.md) | Inline span compiler in the rules layer | Accepted |
| [0012](0012-table-metrics.md) | Table-backed font metrics; `metricsHash` in the page result | Accepted |
| [0013](0013-line-level-vertical-layout.md) | Line-level vertical layout; widows, orphans, vertical glue | Accepted |
| [0014](0014-typography-refinements.md) | Trim scaling, pair compression, hanging punctuation | Accepted |
| [0015](0015-page-masters.md) | Page masters: running header, footer, folio | Accepted |
| [0016](0016-engine-config.md) | `soma.engine/1` engine configuration; preset + window + vertical knobs | Accepted |
| [0017](0017-table-grids-and-headers.md) | Table grids (rowspan), atomic span groups, repeated headers | Accepted; band rule superseded by 0018 |
| [0018](0018-table-continuation-bands.md) | Table continuation by row bands; safe row boundaries; header reuse per part | Accepted |
| [0019](0019-table-rules-and-within.md) | Template-level table rules; `within` selectors for cell context | Accepted |
| [0020](0020-table-column-rules.md) | Table column rules, cell-border grids, dashed rules, rule color | Accepted |
| [0021](0021-page-masters.md) | Named page masters selected by sections; shared content box | Accepted |
| [0022](0022-bold-faces.md) | Bold faces in the metrics table; weight-based face selection | Accepted |
| [0023](0023-publish-v1.md) | Chinese publishing defaults: `publish-v1`, half-em opening trim | Accepted (provisional) |
| [0024](0024-html-frontend.md) | HTML frontend: semantic whitelist, tolerant parser, table grid normalization | Accepted |
| [0025](0025-latex-frontend.md) | LaTeX frontend subset: sections, inline commands, environments, tables, math source | Accepted |
| [0026](0026-math.md) | Display math: `soma-math` box model, metric adapter, `MathBox` in the page result | Accepted |
| [0027](0027-inline-math.md) | Inline math as object atoms: kind-aware metrics hooks, markers, `inlineMath` in the page result | Accepted |
| [0028](0028-toc-and-section-start.md) | Template-driven table of contents (fixed-point page numbers) and a section-start master | Accepted |
| [0029](0029-cli.md) | `soma` CLI: one source + one template to HTML/SVG/canonical JSON | Accepted |
| [0030](0030-math-completion.md) | Math completion: stretched fences, matrices/aligned, math metrics face, text-style constants | Accepted |
| [0031](0031-alignment-and-cover.md) | Alignment as a layout pass (`left` in the kernel, `center`/`right` shifted), generated cover page | Accepted |
| [0032](0032-numbering-and-front-matter.md) | Heading levels, generated numbers, front matter, TOC lists | Accepted |
| [0033](0033-captions-outline-index.md) | Caption styles, wrapping contents rows, outlines and indexes | Accepted |
| [0034](0034-pdf-backend.md) | PDF output with embedded TrueType subsets, bookmarks and page labels | Accepted |
| [0035](0035-metrics-from-pdf-font.md) | Layout metrics can come from the same font the PDF embeds | Accepted |
| [0036](0036-line-end-punctuation.md) | Line-end punctuation pays only for its ink | Accepted |
| [0037](0037-punctuation-ink.md) | Punctuation ink from the outline, not a class constant | Accepted |
| [0038](0038-glyph-edges.md) | Every glyph edge, not just a mark's | Accepted |
| [0039](0039-cff-outlines.md) | CFF outlines in the PDF backend (CID-keyed) | Accepted |
| [0040](0040-name-keyed-cff.md) | name-keyed CFF promoted to CID-keyed; `seac` expanded | Accepted |
| [0041](0041-space-shrink-and-overfull.md) | A space gives up a third; an overfull line is not a break | Accepted |
| [0042](0042-no-fitness-dp.md) | Fitness demerits are not worth four times the DP | Accepted |
| [0043](0043-pdf-flate.md) | PDF streams are deflated; fixed Huffman | Accepted; superseded in part by 0045 |
| [0044](0044-decorations-and-links.md) | Underlines and links cross the layers as atom attributes | Accepted |
| [0045](0045-dynamic-huffman.md) | The deflate encoder builds a dynamic Huffman table when it pays | Accepted |
| [0046](0046-compact-cff-subset.md) | A CFF subset is compacted, and the charset carries the CIDs | Accepted |
| [0047](0047-images-and-tagged-pdf.md) | Figures carry image XObjects; every page is tagged | Accepted |
| [0048](0048-inline-images-in-html-and-svg.md) | HTML and SVG inline a figure as a `data:` URI | Accepted |

## Rules

- An accepted ADR is not edited to change its decision; add a new ADR that
  supersedes it.
- Every ADR states context, decision, consequences, alternatives, and the code
  or documents it aligns with.
- Architecture changes require a new ADR before code changes.
- Historical proposals live in `docs/archive/` and are not normative.
