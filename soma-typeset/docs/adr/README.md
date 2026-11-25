# Architecture Decision Records

Status: **frozen on 2026-11-20** with `docs/architecture-v2.md`.
ADR-0010 amends §15.1 (one extra crate) and fixes shapes that
architecture v2 left open; ADR-0012…0031 extend the implemented system
(table metrics, line-level pagination, typography, page masters, engine
configuration, table continuation, table rules and context selectors,
frontends, display and inline math, contents pages, the CLI, math fences and
matrices, alignment, generated covers, images and tagged PDF, table structure,
trim caps, family-aware faces, baseline propagation, the rest of the math
subset, code-block atoms, equation tags, assembled braces, and container
indentation)
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
| [0049](0049-svg-coordinate-units.md) | SVG coordinates are layout units; the root viewport is pixels | Accepted |
| [0050](0050-table-structure.md) | A table is a real structure tree: Table, TR, TD with spans | Accepted |
| [0051](0051-trim-caps-are-a-budget.md) | A trim cap is a budget, not a measurement | Accepted |
| [0052](0052-family-aware-faces.md) | A style picks its face by family, not by slot | Accepted |
| [0053](0053-math-baseline-propagation.md) | Every math node places itself at the baseline it is given | Accepted |
| [0054](0054-math-subset-completion.md) | Italic variables, `array` formats, `\substack`, braces by ink, `\color`, equation numbers | Accepted |
| [0055](0055-code-block-atoms.md) | One atom table per code block; a code line keeps its spaces | Accepted |
| [0056](0056-tags-text-math-and-braces.md) | `\tag`/`\notag`, maths inside `\text`, braces assembled from rules when wide | Accepted |
| [0057](0057-containers-indent-their-children.md) | A container indents its children; a list item carries its marker | Accepted |
| [0058](0058-hanging-columns-and-dashes.md) | Hanging indents, content-sized table columns, wide dashes, cells as inlines | Accepted |
| [0059](0059-hyphenation.md) | Hyphenation: a boundary kind, an external pattern set, and a hyphen charged to the line | Accepted |
| [0060](0060-ruby.md) | Ruby annotations: one base atom, line ascent reserved, geometry resolved in layout | Accepted |
| [0061](0061-index-structure-and-order.md) | Index tree, `see`/`see also`, pinyin order from a checked-in table | Accepted |
| [0062](0062-markdown-structures.md) | Nested lists, GFM tables, and setext headings in Markdown | Accepted |
| [0063](0063-docx-reader.md) | Word reader: dependency-free zip/inflate + OOXML to DocIR | Accepted |
| [0064](0064-line-rhythm.md) | K7 line rhythm: four looseness classes as DP state, opt-in `rhythm-v1` | Accepted |
| [0065](0065-column-shares.md) | Relative column widths (`Column.share`) and HTML `<colgroup>` | Accepted |
| [0066](0066-word-styles-and-media.md) | Word styles resolved through `basedOn`; embedded images as figures | Accepted |
| [0067](0067-latex-macros.md) | LaTeX macros and counters expanded before parsing | Accepted |
| [0068](0068-figure-units-and-image-sizes.md) | Figure/caption units, table headers that never stand alone, intrinsic image sizes | Accepted |
| [0069](0069-latex-article-defaults.md) | The built-in template is LaTeX `article` 10pt; `latex-v1` preset | Superseded by ADR-0088; design archived |
| [0070](0070-math-v2.md) | Math v2: TeX atom classes and spacing, `MATH` constants + glyph ink, style levels, inline math breaking | Accepted |
| [0071](0071-web-kp-slice.md) | Browser slice: `soma-core` + `soma-rules` behind a C ABI; three defaulted `Metrics` methods; no wasm-bindgen | Accepted |
| [0072](0072-emergency-window.md) | The emergency stage gets a width window: 1.26M → 30k candidates at 20 em, same breaks (D94) | Accepted |
| [0073](0073-cff-ink.md) | CFF faces record punctuation ink: a bbox-only Type 2 interpreter fills the gap that made a trailing comma sit 0.28 em inside | Accepted |
| [0074](0074-block-runs.md) | Blocks cover runs, not holes: exact coverage without a per-glyph list; the embedded table regenerates at 288 KB (D96) | Accepted |
| [0075](0075-math-variants.md) | Math variants: display operators, radicals and fences use the font's designed glyphs; radicals connect and the degree sits in the nook (D93) | Accepted |
| [0076](0076-math-typst-alignment.md) | Typst-aligned math geometry: radical clearance, axis-centred operators/matrices, fraction shifts, combining accents with top attachments (D93, D98) | Accepted |
| [0077](0077-default-math-face.md) | The default math face is probed and proved against the metrics table: at least 16 exact advances, a warning when none matches, no font redistributed | Accepted; default face superseded by 0079 |
| [0078](0078-latin-modern-math.md) | The default math face is Latin Modern Math; CFF math ink, Type 2 curve/stem fixes, the regenerated tables and the bundled face | Accepted; default face superseded by 0079 |
| [0079](0079-new-computer-modern-math.md) | The default math face is New Computer Modern Math: the Typst reference face, bundled and proved against the regenerated tables | Accepted |
| [0080](0080-display-math-skip.md) | A display formula gets one body em before/after by default, even when a template does not style `mathBlock` | Accepted |
| [0081](0081-plex-reference-template.md) | A reference page design transcribed from pandoc `soma.latex`: Plex Sans body, zero indent, 20pt skip, booktabs rules | Accepted; promoted to the built-in default by ADR-0088 |
| [0082](0082-line-start-opening-trim.md) | A justified line trims the side bearing of an opening mark at its start | Accepted; supersedes ADR-0036 §4 |
| [0083](0083-footnote-area.md) | The footnote area gets a separator rule, a 1.5em number column, and a layout-unit superscript raise | Accepted |
| [0084](0084-table-rule-separation.md) | Table rules own the vertical air: 1/4em above/below rules, table `space-before/after` from the template, no per-cell padding | Accepted |
| [0085](0085-continuous-decoration.md) | An underline run bridges justification glue; a decorated atom extends to its decorated neighbour | Accepted |
| [0086](0086-half-leading.md) | Renderers split line leading half above/below the baseline; metrics can zero a font's lineGap | Accepted |
| [0087](0087-natural-table-width.md) | `table: width: natural` centres a content-width table; GFM dash counts become column shares | Accepted |
| [0088](0088-plex-builtin-default.md) | The Plex reference is the built-in template; LaTeX article is archived as a selectable template | Accepted; supersedes ADR-0069 |
| [0089](0089-monospace-family.md) | Monospace is a third family: `FAMILY_MONO`, PDF `F7`/`F8`, `--font-mono`, sans fallback | Accepted |
| [0090](0090-code-block-panel.md) | Code blocks are full-measure panels: template `background`/`padding-*`, geometry resolved before pagination | Accepted |
| [0091](0091-syntax-highlighting.md) | A dependency-free line lexer emits `SyntaxClass`; colors come from the template `syntax:` palette | Accepted |
| [0092](0092-tikz-vector-subsystem.md) | TikZ is a vector subsystem: `soma-tikz` parses the core PGF drawing language into integer paths on `MathBox.paths`; unsupported syntax fails loudly | Accepted |
| [0093](0093-web-studio.md) | Web Studio is all-Rust wasm (Leptos CSR) plus an Axum I/O boundary; the whole engine runs in the browser and canonical hashes must match | Accepted |
| [0094](0094-html-geometry-percent.md) | HTML geometry is percentage-based relative to the page/block frame; inline sizes are ratios, not nested em | Accepted |
| [0095](0095-studio-builds-the-active-tab-frontend.md) | Studio builds each tab with its own frontend; `BuildRequest.format` is optional and falls back to the project input | Accepted |
| [0096](0096-footnote-mark-attachment.md) | A footnote mark attaches to the text before it (no break, no gap); body superscripts are 2/3 size, raised 2/5 em | Accepted |
| [0097](0097-display-math-page-frame.md) | Display math HTML percentages resolve against the page; inline math keeps the block frame | Accepted |
| [0098](0098-short-display-skip.md) | A display after a short line keeps half the template's space-before and 70% of its space-after | Accepted |
| [0099](0099-radical-variant-measurement.md) | CFF variant ink drops the optional Type 2 width; HTML/SVG place the fallback glyph at the variant's ink top and advance | Accepted |
| [0100](0100-studio-host-binding.md) | `--host` opts Studio out of loopback; the `/api` Origin allowlist follows the bound host while `cross-site` stays refused | Accepted |
| [0101](0101-studio-hyphenation.md) | Studio loads TeX hyphenation patterns from `--hyphenation` or `hyphen-en.pat` and feeds native and wasm builds | Accepted |
| [0102](0102-diagram-compiler.md) | Mermaid/DOT are `Block::Diagram`; `soma-diagram` compiles them to the TikZ vector box, with a strict source fallback and a TikZ export | Accepted |
| [0103](0103-studio-tikz-export.md) | Studio exports the first compiled diagram as natural-size TikZ through `/api/tikz`; page scaling and figure selection stay out of the source export | Accepted |
| [0104](0104-mixed-markdown-latex-html.md) | Markdown is a compound frontend: math, LaTeX commands/environments and semantic HTML map into the same DocIR through the peer frontends | Accepted |
| [0105](0105-ragged-line-opening-trim.md) | A ragged or left-aligned line also trims an opening mark at its start; centered/right text undoes the optical trim | Accepted; extends ADR-0082 |
| [0106](0106-book-layout-pass.md) | Book pass: free segment tails, chapter-only running heads, `layout.mode`, `book-v1` 3 em short-last, contents/cover styles, figure policy | Accepted; closes D110 |
| [0107](0107-markdown-section-and-footnote.md) | Markdown delegates block section commands and lifts inline `\footnote` into real definitions; `\index` inside a note is collected | Accepted; closes the footnote part of D109 |
| [0108](0108-math-font-commands-and-details.md) | Font-selection math commands are transparent, matrices get a half-em gutter, `<details>/<summary>` are semantic blocks | Accepted |
| [0109](0109-svg-proof.md) | The Studio proof is SVG rendered from `PagedLayout`; PDF stays download-only | Accepted; implements `docs/proposal-svg-proof.md` |
| [0110](0110-colophon-block.md) | The colophon is a ruled multi-line block; `\colophon{a\\b}` splits lines | Accepted |
| [0111](0111-inline-code-and-separator-breaks.md) | Inline code breaks at its spaces and separators (`CODE_BREAK_PENALTY`); Latin `-`/`/` breaks are priced (`WORD_BREAK_PENALTY`); the Studio defaults to `book-v1` | Accepted; addresses the TACTUS line-breaking review |
| [0112](0112-code-intrusive-breaks-and-short-lines.md) | Long unseamed code runs split per character (`INTRUSIVE_BREAK_PENALTY`); camelCase/`_`/`.` are seams; the short-line charge covers non-final lines; `modelVersion` 4 / `rulesVersion` 2 | Accepted; closes D114 and D115 |
| [0113](0113-space-coverage-and-diagram-fit.md) | The metrics face covers its own space; layout and PDF fall back to the math face for glyphs the text face lacks; wide diagrams fit their label caps instead of shrinking | Accepted; fixes the TACTUS review's glyph and diagram findings |
| [0114](0114-default-plex-template-v2.md) | Plex template v2: airier table cells, a deliberate contents page, a muted cover and a centred colophon; the built-in default follows | Accepted |

## Rules

- An accepted ADR is not edited to change its decision; add a new ADR that
  supersedes it.
- Every ADR states context, decision, consequences, alternatives, and the code
  or documents it aligns with.
- Architecture changes require a new ADR before code changes.
- Historical proposals live in `docs/archive/` and are not normative.
