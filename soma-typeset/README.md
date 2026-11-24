# soma-typeset

**A runtime-independent typesetting core for Chinese-first, mixed CJK/Latin text.**

This workspace contains the verified Phase 1 inline kernel (K1–K8) and the
frozen architecture for the document/template/page phase. It is a clean-room
implementation of the PRD in [`../prd.md`](../prd.md); it is deliberately
independent of Typst, TeX, DOM, the filesystem, and platform font APIs.

## What works now

- Fixed-point geometry: `Width(i32)` in 1/4096 em; no `f32` in the core.
- The frozen inline kernel: `ParaIR` (`Atom` + `Boundary`), metrics seam,
  K-P/greedy line breaking, and canonical line results.
- A frozen Phase 2 architecture: DocIR (`soma-doc`), pure-YAML templates,
  Layout Box Tree (`soma-layout`), vertical pagination and footnotes.
  `soma-doc` is implemented; the remaining layers are designed and recorded in
  ADRs.
- Greedy-F (furthest fit), Greedy-C (same cost model, no lookahead), and a
  reference Knuth-Plass DP.
- Forbidden breaks, glue stretch/shrink, first-line indent, last-line raggedness,
  and a short-last-line penalty.
- Canonical, integer-only `LayoutResult` JSON with a deterministic byte output.
- K1 parameter contract: `default-v1` preset, validation, and a deterministic
  parameter hash; non-Justify alignment is rejected with `UnsupportedAlign`.
- K1 cost model: normalized `line`/`badness`/`break_penalty`/`short_last`
  terms, plus deterministic tie-break and a brute-force reference test.
- K2 boundary evidence: `BoundaryRole`, `Line.boundary_deviation`,
  `Line.cjk_latin_adjustments`, and `break_paragraph_traced` for per-line
  explanation.
- K3 line boxes: `GlyphMetrics`, `Line.ascent/descent/leading/height`, inline
  large atoms, baseline shifts, and optional `height_cost`; canonical schema is
  `soma.layout/3`.
- K4 edge trimming: `Atom.compress_left/right` → `Line.trim_left/trim_right`,
  deterministic end-before-start trimming, `Line.ratio_milli`, compression
  cost, and the rendering coordinate convention; canonical schema is
  `soma.layout/4`. The rule compiler now gives opening, closing, and middle
  punctuation their side-bearing trim caps.
- K5 forced breaks: `Breakability::Forced` pre-splits a paragraph into
  independently broken segments. Forced segment ends are not stretched, do not
  pay `short_score`, and are marked with `Line.forced_end`; the next segment
  does not repeat the first-line indent. Canonical schema is
  `soma.layout/5`.
- K6 performance/reporting: `LayoutResult` emits `breaker_used`,
  `fallback_stage`, `length_limited`, `emergency_lines`, and
  `forbidden_violations`; a conservative atom-count window keeps the DP inside
  the native budget; the fallback ladder now includes forbidden rescue. The
  deterministic `max_kp_atoms` policy is available but remains `None` by
  default because 4,000 atoms are inside budget. Canonical schema is
  `soma.layout/6`.
- K8 integration: `soma-eval` provides benchmarks, sensitivity sweeps, and
  blind-test statistics; `soma-demo` renders a self-contained HTML page that
  consumes `LayoutResult` without browser re-breaking; `soma-wasm` is a
  WASI/native fixture used to prove byte-identical canonical output across
  targets. Scripts: `scripts/bench.sh`, `scripts/sweep.sh`, `scripts/demo.sh`,
  `scripts/wasm-hash.sh`.
- A minimal CJK/Latin rule compiler: CJK atoms, Latin runs, spaces as glue,
  punctuation classes, basic 避头尾 (forbidden line-start/line-end) rules,
  edge trim caps, and `paragraph_from_lines` for hard-broken source lines.
- A1 DocIR (`soma-doc`, schema `soma.doc/2`): 13 block and 17 inline kinds with
  `NodeId`/`SourceSpan`/`Attrs`, footnotes, figures, tables, math source, raw
  payload preservation, structural validation, canonical JSON, and two hashes
  (`docHash` for exact reproduction, `contentHash` for content identity). The
  crate has **zero dependencies** and no floats; Gate A enforces that.
- A3–A6 pipeline: `soma-layout` builds paragraph boxes over the frozen kernel and
  paginates with **line-level vertical K-P** by default (`soma.pages/10`):
  paragraphs split across pages, widows/orphans are penalties, vertical glue can
  stretch, footnotes are charged to the marker's line, and page masters add a
  running header, footer, and folio. Figure boxes, table cell boxes,
  code/rule boxes, display-formula boxes, inline-formula atoms, and a `TextSource`
  seam complete the tree;
  `soma-md`, `soma-html`, and `soma-latex` parse documented Markdown/HTML/LaTeX
  subsets into DocIR;
  `soma-render-html`
  paints absolutely positioned HTML. The WASI fixture hashes DocIR, inline
  layout, and pages together.
- E1–Q′ effect layer: `soma-metrics` ships a committed `soma.metrics/7` table
  generated from Noto CJK SC + New Computer Modern Math (per-glyph ink, MATH
  constants and vertical variants), `StyledMetrics` scales
  advances by the resolved font size, trim follows the atom's size, adjacent
  punctuation compresses, closing punctuation can hang, and a baseline grid is
  available per template. `soma-render-svg` is a second renderer used by Gate R;
  `soma-eval score` prints the effect scorecard.
- Real corpus: `corpus/zh/` holds nine prose documents (technical note,
  notice, abstract, essay, mixed-script notes, public-domain classics,
  footnote-dense note) plus table (colspan/rowspan/repeated header), real display-math, and 30-page book fixtures.
  `soma-eval corpus` measures them and `soma-eval calibrate` sweeps tolerance ×
  badness scale on a proxy objective; see
  [`docs/corpus-calibration.md`](docs/corpus-calibration.md).
- Display math (`soma-math`, ADR-0026/ADR-0054/ADR-0056): a TeX subset (symbols,
  italic variables, scripts, fractions, radicals, `\sum`/`\int` with limits,
  function names, `\left…\right`, `array` column formats, `\substack`,
  `\overbrace`/`\underbrace` — glyph when narrow, assembled from rules when
  wide — `\color`, tags) laid out into integer glyphs and rules. `soma-layout`
  centres one `MathBox` per `MathBlock`; both renderers paint it; a malformed
  formula falls back to text instead of failing the page. `numbering.equations`
  numbers `equation`/`align`/`gather`/`multline` blocks per row, right-aligned in
  the margin, with `\tag`/`\tag*`/`\notag` overriding a row; a nested `aligned`
  never adds numbers. `\text{}` accepts `$…$` inside it. Inline formulas are
  object atoms measured by their own box (ADR-0027), so they break and raise the
  line box like any other inline object. Fences stretch to their body, and
  `matrix`/`pmatrix`/`bmatrix`/`vmatrix`/`cases`/`aligned` are supported; symbols
  come from a dedicated math face in the metrics table (ADR-0030). See
  [`docs/math.md`](docs/math.md).
- TikZ drawings (`soma-tikz`, ADR-0092): the core PGF drawing language —
  coordinates (Cartesian/polar/relative/named), `--`, `rectangle`, `circle`,
  `ellipse`, `arc`, `grid`, `-|`/`|-`, nodes with anchors and labels, `\foreach`,
  colours, line widths, dash patterns and arrow barbs — parsed into integer
  vector paths on the display-math box. The PDF/SVG/HTML backends paint the same
  paths; node text goes through the math subsystem, so `$…$` and CJK work. An
  unsupported key or command is a hard error and falls back to the visible
  source, never a wrong picture. See [`docs/tikz.md`](docs/tikz.md).
- Browser slice (ADR-0071): `soma-web` compiles `soma-core` + `soma-rules` to a
  32.7 KB gzip, **zero-import** `wasm32-unknown-unknown` module; `web/` is a
  dependency-free ES module plus a demo and a self-checking harness. The host
  measures with canvas `measureText`, hands over one integer per atom, and gets
  line ranges and box coordinates back — the same numbers the PDF renderer
  consumes. `scripts/web.sh` verifies zero imports, size, native/wasm parity,
  timing, memory stability and the fallback policy. See
  [`web/README.md`](web/README.md).
- Soma Studio (ADR-0093, ADR-0094): the **whole engine runs in the browser**
  (`soma-studio-ui`, Leptos CSR) and Axum is only an I/O boundary
  (`soma-studio`) for files, fonts, native verification and PDF. The UI has
  source/proof/inspect/verify views, Shadow-DOM pages, tabs, a command palette,
  a theme system, a local-rules assistant, and a font-consistency probe. Both
  sides call `soma-studio-core`; canonical hashes must match, which closes the
  page-geometry half of D95. Typing rebuilds after a 220 ms debounce and IME
  composition defers the build. `scripts/studio.sh` locks S1–S10. See
  [`docs/studio.md`](docs/studio.md).
- Code blocks (`ADR-0055`): each source line keeps its own spaces and is laid
  out on the block's atom table — indentation survives, nothing reflows, and a
  line that is too long overflows instead of wrapping.
- Engine configuration: `soma-engine` parses `soma.engine/1` (preset, window,
  atom window, fallback switches, vertical cost model) into the same
  `BreakConfig`/`VerticalParams` the hand-built path uses. See
  [`docs/engine.md`](docs/engine.md) and ADR-0016.
- Contents pages and chapter openings (ADR-0028/ADR-0032): `toc: {depth, title,
  leaders, figures, tables}` produces an automatically paginated contents page
  with dot leaders and optional lists of figures and tables (page numbers are a
  fixed point over at most four layout passes, reported as `toc.stable`), and
  `page.start-master` selects a master for pages that open a section.
  `numbering:` generates Chinese (`第一章`/`第一节`) or dotted (`1.2.3`) heading
  numbers and `图 {n}`/`表 {n}` caption numbers at layout time; they never enter
  DocIR. `page.front-matter: continuous|roman|hidden` prints the cover and
  contents in roman numerals (or none) and restarts the body at 1.
- Long-document output (ADR-0033): figure/table captions have their own
  elements (`figureCaption`/`tableCaption`); contents rows wrap and keep the
  page number on the last line; the page result carries an `outline` (one
  bookmark per heading with physical and printed page) and an `index` report.
  `\index{term}` in LaTeX or `[[index:term]]` in Markdown, plus
  `index: {title, sort}` in the template, produces an index with merged page
  lists (runs of three or more pages collapse to `3–5`). The marker grammar supports sub-entries (`宋体!字面`), per-part sort
  keys (`宋体@songti`), `|see{}` and `|seealso{}`; markers inside table cells
  and footnote definitions are collected on the page that prints them. Order is
  code point (`codepoint`, the default) or **pinyin** from the checked-in
  `soma-collate` table (ADR-0061).
- Ruby (ADR-0060): `\ruby{汉字}{hànzì}` in LaTeX and `[[ruby:汉字|hànzì]]` in
  Markdown. The base stays one atom, the line reserves space above its ascent,
  and the PDF, SVG, and HTML renderers each draw the annotation.
- Hyphenation (ADR-0059): `soma-hyphen` implements Liang's algorithm over an
  external TeX pattern file (`soma render --hyphenation en.pat`). A broken line
  ends in a real hyphen whose width is charged to the line; a document without
  patterns is byte-identical to before.
- Word input (ADR-0063/ADR-0066): `soma render --input draft.docx` reads the
  archive with a dependency-free zip/inflate and maps paragraphs, headings
  (by style name, through `basedOn` chains), inline emphasis, tables, lists,
  core metadata, and embedded images (as figures) to DocIR. Floats, wrap,
  crop, fields, and footnotes are not read; no layout fidelity is promised
  (remaining Word limits: N3 in `docs/debt.md`).
- Markdown structure (ADR-0062): nested lists, GFM tables (alignment, escaped
  bars), setext headings, and link reference definitions join the existing
  subset. HTML `<colgroup>`/`<col>`
  can give a column a relative width (ADR-0065); the HTML frontend also maps
  definition lists, `<ins>`, `<sup>`/`<sub>`, `<hr>` and figure captions.
- LaTeX macros and counters (ADR-0067): `\newcommand`/`\renewcommand`/
  `\providecommand`/`\def` with up to nine arguments expand before parsing;
  `\newcounter`/`\setcounter`/`\stepcounter` and `\arabic`/`\roman`/
  `\alph`/`\value`/`\the<name>` print counter values. `verbatim` is never
  expanded.
- Line rhythm (ADR-0064): `preset: rhythm-v1` charges a DP state for a change
  of looseness class between adjacent justified lines. `publish-v1` and
  `default-v1` are unchanged.
- One command (ADR-0029): `soma render --input doc.tex --template book.yaml
  --format html|svg|json|pdf` runs the whole pipeline; `--input - --from md`
  reads stdin, and `--input <dir> --out <dir>` batches a directory. `soma check`
  prints layout facts and the canonical hash. See [`docs/cli.md`](docs/cli.md).
- Math v2 (ADR-0070): TeX's eight atom classes and 8×8 spacing table
  (`\sin x`, `a,b`, `x = -1`), four style levels with cramping and rule-18
  scripts, and geometry taken from the math face's `MATH` table plus per-glyph
  vertical ink: fraction shifts and gaps, a vinculum on the radicand's ink,
  `\left…\right` sized by ink, accents, `\binom`, `\operatorname`, and
  inline formulas that break at relations and binaries. `soma.metrics/7`
  (MATH constants, vertical variants and per-glyph ink).
- Plex reference defaults (ADR-0088): without `--template` the CLI uses the
  pandoc `soma.latex` page design — A4, 25.4 mm margins, IBM Plex Sans SC
  10 pt on a 14.4 pt baseline, zero paragraph indent, a 20 pt paragraph skip,
  natural-width booktabs tables, a cover from metadata, a TOC from headings,
  and the Rust-book code panel. The former LaTeX `article` default is
  archived as `examples/templates/latex-article.yaml`; without `--engine` the
  CLI still breaks with `latex-v1` (`tolerance=1260`, ragged last line).
  An explicit template or engine file still wins.
- Code blocks (ADR-0089/0090/0091): a full-measure panel with a template
  `background` and `padding-*`, a real monospace family (`mono`, PDF
  `F7`/`F8`, `--font-mono`), and a dependency-free line lexer for Rust, C,
  C++, shell, Markdown, LaTeX, Python, JS/TS, JSON, YAML, TOML, HTML, CSS,
  SQL, Go, Java, Kotlin, Swift, C#, Ruby, PHP, Lua and Haskell. Token colors
  come from the template's optional `syntax:` palette; the Plex template
  transcribes mdBook's Rust theme.
- PDF output (`soma-render-pdf`, ADR-0034): `--format pdf --font book.ttf`
  embeds a sparse TrueType subset (Identity-H + ToUnicode), writes integer
  glyph positions, table rules, display math, a nested bookmark tree from
  `outline`, and `/PageLabels` from the printed labels. The same layout and
  fonts produce identical bytes. `--metrics` lets layout measure with a table
  generated from that same font (`tools/metrics-gen`); `scripts/book.sh` does
  this for the example. `glyf` and CFF outlines are embedded (`/FontFile2` and
  `/FontFile3`; a name-keyed CFF is promoted to CID-keyed and its `seac`
  composites are drawn out). `--font-math` names the formula face; when it is
  omitted and the document has a formula, the CLI uses the bundled New Computer
  Modern Math (the embedded table's face), then probes installed faces, and
  accepts only one whose advances match the table. So a fresh install gets
  serif LaTeX-style formulas with no flag and no TeX Live; `∑` and `√` are never
  drawn with the body face by accident (ADR-0077). Streams are
  deflated by a deterministic encoder
  that picks fixed or dynamic Huffman blocks per stream (ADR-0043/ADR-0045),
  underlines and strikethroughs are drawn from the font's `post` table, links
  become `/Annots` rectangles (ADR-0044), figures embed PNG and JPEG images as
  XObjects, and every page carries a tagged structure tree with `/Alt` text
  (ADR-0047). The HTML and SVG renderers inline the same images as `data:`
  URIs, so a preview stays one self-contained file (ADR-0048).
- Full-capability sample (ADR-0068): `examples/book/full.tex` +
  `scripts/full.sh` render one 14-page book with every front-end and layout
  feature in a single document — ruby, hyphenation, nested and description
  lists, a table with merged cells, PNG/JPEG figures at their true aspect
  ratio, numbered formulas, macros and counters, footnotes, and a pinyin index
  with sub-entries, `see`/`see also`, and page ranges. `full-html.pdf` is a
  one-page supplement for the HTML-only constructs (`<colgroup>` widths and
  native `<ruby>`).
- Reference page design (`examples/templates/plex.yaml`, ADR-0081/0088): a
  pure-YAML transcription of pandoc's `soma.latex` — A4 with one-inch margins,
  IBM Plex Sans SC at 10pt on a 1.44 baseline, zero first-line indent with a
  20pt paragraph skip, article-sized headings, italic quotations, booktabs
  three-rule tables and a pinyin index. It is also the built-in template.
  `scripts/plex.sh` resolves the system fonts (they are not redistributed),
  measures the body, mono and math faces with `tools/metrics-gen`, and renders
  `examples/book/full.tex` to `plex.pdf` plus the multi-language code sample
  `examples/templates/code.md` to `code.pdf`.
- First-release artifact (`examples/book`, ADR-0031/ADR-0032/ADR-0033): `scripts/book.sh`
  renders an eleven-page book from `examples/book/book.tex` + `book.yaml` — generated
  cover, roman-numbered front matter, contents with leaders plus figure/table
  lists, numbered chapters and sections, a figure with a caption, a formula, a
  three-line table with a caption, a footnote, a seven-entry index, and a PDF
  with ten bookmarks; `scripts/book.sh` also writes `book.pdf` when a TrueType
  font is available. Alignment
  (`left`/`center`/`right`) is a layout pass, so titles and cover lines can be
  centred. The build has 0 overflow pages and 0 degraded lines;
  `docs/first-release.md` lists the remaining design gaps.
- Delivery brief (`examples/book/brief.tex`, `scripts/brief.sh`): a twelve-page
  PDF about the engine itself — cover, contents with figure and table lists,
  bulleted capability lists, numbered formulas with a `\tag` and a `\notag`
  row, a code block with a pipeline diagram, two images, a data table, a
  quotation, and an index — written in the LaTeX subset and set with the Song
  template. It is the shortest end-to-end exercise of everything above.
- Blind-test tooling: kit generation, key unmapping, Wilson interval, Fleiss'
  kappa, and a committable Markdown report (`soma-eval blind --out`). A
  **simulated** dry run proves the chain (`docs/blind-dry-run.md`); the human
  panel (D01) is still open.
- A2 template layer (`soma-template`, schemas `soma.template/9` and
  `soma.styles/7`): a documented YAML subset (block + single-line flow
  collections) with line/column errors, exact units (decimal → rational →
  1/4096 em, truncation toward zero; percentages only where a base exists), a
  selector cascade with `within`/`parent`/`ancestors` and heading levels, a
  25-property whitelist, data-level `parse_template_inherit`, `templateHash`,
  and `StyledDoc` as lifetime-free data with `styleHash`. Templates cannot write
  engine knobs; Gate A enforces that statically.

## What is not here yet

- The human blind-test run (D01; Gate Q remains `Inconclusive`), incremental
  layout (N4), nested box trees (D51), page/chapter search windows (D66), and
  the wider TikZ library surface (D99). Browser WASM packaging is delivered
  (`web/`, ADR-0071). See [`docs/debt.md`](docs/debt.md).

## Limitations

- Font metrics come from a committed table, not from shaping: no kerning pairs
  yet (D100), CJK has no italic face (D61), and rare codepoints fall back to 1em
  (D63).
- Hyphenation needs a pattern set supplied by the caller: there is no built-in
  dictionary, and a run that the patterns do not cover can still compress past
  its shrink limit. Tracking is opt-in per style, not automatic for hard cases
  (D60).
- The punctuation hard rules are cited against GB/T 15834-2011 via W3C clreq
  §6.1.1 (basic). Trim and pair-compression ratios remain `ParamOnly`; D04
  measured the alternatives and kept the current ratios for quality reasons.
- Page masters share one content geometry per document; per-master content
  geometry is not modeled (D62). Header/footer text truncates with `…` when it
  exceeds the margin, and does not spill into the content box.
- The demo uses the real metrics table, but its HTML is still a geometry proof:
  it paints positions and lets the browser choose the font.

## Layout

```text
crates/
  soma-core/   # frozen inline kernel: units, ParaIR, metrics, breaker, result
  soma-doc/    # DocIR: nodes, source spans, validation, canonical JSON, hashes
  soma-engine/ # soma.engine/1: preset, window, fallback, vertical knobs
  soma-metrics/ # soma.metrics/7 parser + embedded Noto CJK / NewCM Math table
  soma-template/ # pure-YAML template: parse, exact units, cascade, StyledDoc
  soma-layout/ # paragraph boxes, line-level pagination, page masters
  soma-md/     # Markdown subset -> DocIR
  soma-html/   # HTML subset (semantic whitelist) -> DocIR
  soma-latex/  # LaTeX subset (sections, inline, environments) -> DocIR
  soma-docx/   # Word .docx (zip + OOXML) -> DocIR
  soma-collate/ # pinyin order for the index
  soma-hyphen/ # Liang hyphenation over an external pattern set
  soma-cli/    # `soma`: source + template -> HTML/SVG/PDF/canonical JSON
  soma-math/   # TeX subset -> integer math boxes (symbols + rules)
  soma-tikz/   # core PGF drawing language -> integer vector paths
  soma-render-html/ # PagedLayout -> absolutely positioned HTML
  soma-render-svg/  # PagedLayout -> SVG (Gate V/R)
  soma-render-pdf/  # PagedLayout -> PDF (TrueType subset, bookmarks)
  soma-rules/  # text -> ParaIR (Chinese punctuation + CJK/Latin segmentation)
  soma-highlight/ # dependency-free code lexer: SyntaxClass spans per line
  soma-eval/   # benchmark, sweeps, blind-test statistics, effect scorecard
  soma-demo/   # static HTML renderer consuming LayoutResult
  soma-web/     # C ABI slice for the browser (rules + K-P, no imports)
  soma-wasm/   # native/WASI fixture for cross-target byte verification
  soma-studio-core/ # shared Studio build: parse, layout, fragments, probes, DTOs
  soma-studio-ui/   # Leptos CSR workbench; the whole engine compiled to wasm
  soma-studio/      # Axum boundary: files, assets, fonts, verify, PDF, SSE
tools/dry-run-panel/ # simulated blind-test panel generator (not human data)
tools/
  metrics-gen/ # offline OTF/TTC -> soma.metrics/7 (numbers + ink)
examples/
  book/        # first-release book: book.tex + song.tex + brief.tex (+ .yaml)
corpus/
  zh/          # self-authored evaluation corpus (five genres)
  README.md    # provenance and corpus rules
scripts/
  gates.sh     # fmt, clippy, tests, wasm hash, demo smoke, dependency guard, gates
  gate-a.sh    # architecture gate: layer boundaries, DocIR purity, schema versions
  gate-c.sh    # content gate: capability matrix, degradation, renderer safety
  gate-r.sh    # Gate V/R: SVG visual lock + cross-renderer atom parity
  gate-q.sh    # Gate Q': effect scorecard tests + report
  corpus.sh    # corpus metrics + calibration sweep
  effect-score.sh # alias for gate-q.sh
  bench.sh     # P50/P95 budget check
  sweep.sh     # tolerance x badness sweep
  demo.sh      # target/demo.html
  wasm-hash.sh # native vs wasm32-wasip1 canonical bytes
  kp-verify.sh # K-P closure: gates + bench budget + length policy
  studio.sh    # S1–S10: browser self-test, CSP, PDF pages, SSE reloads, font probe
web/
  soma-kp.js       # ES module: compile / measure / break / render (no dependencies)
  soma-kp.wasm     # built slice (32.7 KB gzip, zero imports) — rebuilt by scripts/web.sh
  soma-kp.worker.js
  demo.html        # interactive demo: width, size, family, indent, canvas mode
  harness.html     # self-checking page used by the gate
  parity.txt       # fixture shared by the Rust ABI test and the browser harness
docs/
  soma-atlas.html     # single-file research atlas: capability map, gates, numbers, pitfalls
  architecture-v2.md  # frozen architecture
  fonts.md            # how a body font is scored and a metrics table is built
  mixed-font.md       # coverage, rescue faces, and the fallback plan
  font-preset.md      # proposal: ink-aware line width, one profile per font
  adr/                # accepted decisions (ADR-0001 … 0092)
  roadmap-v2.md       # A1–A6 + E1–E7 + Q′ schedule, work packages, gates
  debt.md             # global not-done register (D01–D100, N1–N6)
  proposal-effect-max.md # E1–E6 + Q′ proposal
  proposal-web-kp.md  # Web v1: the K-P slice in wasm (measurement, ABI, budgets)
  e1-q-report.md      # E1–Q′ delivery report
  engine.md           # soma.engine/1: shape, defaults, errors, usage
  corpus-calibration.md # real corpus: metrics and proxy calibration
  docir.md            # DocIR contract: nodes, canonical JSON, validation
  math.md             # display math: TeX subset, box model, style constants
  tikz.md             # TikZ subset: coordinates, paths, nodes, backends, limits
  template.md         # template DSL: YAML subset, units, cascade, hashes
  a1-report.md        # A1 milestone report (Gate A1 checklist)
  a2-report.md        # A2 milestone report (Gate A2 checklist)
  a3-a6-report.md     # A3–A6 report: layout, pagination, Markdown, HTML, footnotes
  gate-c-and-vertical-kp.md # Gate C and vertical K-P report
  capabilities.md     # content capability matrix (Gate C)
  kp-closure.md       # K1–K6/K8 closure report (Gate T checklist)
  archive/            # historical, non-normative
```

## Architecture

The next phase is defined by [`docs/architecture-v2.md`](docs/architecture-v2.md)
(frozen 2026-11-20) and the ADRs in [`docs/adr/`](docs/adr/README.md):

- `soma-doc` owns a clean-room DocIR; `soma-core` stays the inline kernel;
- templates are pure YAML (manifest + style rules), separate from engine config;
- `soma-layout` builds the Layout Box Tree and paginates with vertical K-P;
- Markdown/HTML/LaTeX front ends only produce DocIR;
- renderers consume `LayoutResult` and never re-break.

Order and estimates live in [`docs/roadmap-v2.md`](docs/roadmap-v2.md)
(A1–A6). Everything not yet built is tracked in
[`docs/debt.md`](docs/debt.md).

## Try it

```sh
cargo run -p soma-rules --example layout
```

This prints a small mixed CJK/Latin paragraph as lines produced by
Knuth-Plass and both greedy baselines, followed by the canonical layout JSON.

For the visual demo and evaluation harness:

```sh
scripts/demo.sh            # writes target/demo.html
scripts/web.sh             # build the browser slice + W1-W6 gates
scripts/web-bundle.sh      # -> target/soma-kp-standalone.html (open it, no server)

# Soma Studio (all-Rust wasm UI + Axum boundary)
(cd crates/soma-studio-ui && trunk build --release)
cargo run -p soma-studio -- --input examples/book/full.tex \
  --template examples/templates/plex.yaml --metrics examples/templates/out/plex.smt \
  --font ~/.fonts/IBMPlexSansSC_Regular.ttf --font-bold ~/.fonts/IBMPlexSansSC_Bold.ttf \
  --port 7070
scripts/studio.sh          # S1–S10 (starts its own server)
(cd . && python3 -m http.server 8000)   # or browse /web/demo.html
scripts/wasm-hash.sh       # native vs wasm32-wasip1 canonical bytes
scripts/bench.sh           # 100..4,000 atoms, P50/P95, budget check
scripts/sweep.sh           # tolerance x badness scale
cargo run -p soma-eval --release -- inspect --kind mixed --atoms 500
cargo run -p soma-eval --release -- blind preferences.csv
```

## Test

```sh
scripts/gates.sh
scripts/studio.sh   # S1-S10 for the web workbench (needs chromium)
```

The gate runs format, clippy, 924 tests, a `wasm32-unknown-unknown` check for
`soma-core`, `soma-doc`, `soma-engine`, `soma-metrics`, `soma-highlight`, `soma-template`,
`soma-layout`, `soma-tikz`, `soma-md`, `soma-render-html`, `soma-render-svg`, and
`soma-render-pdf`, a
native/`wasm32-wasip1` canonical-byte comparison (DocIR + inline layout + pages),
a demo smoke render, the `soma-core` dependency-boundary check, Gate A, Gate C,
Gate V/R (SVG visual lock + cross-renderer atom parity), and Gate Q′ (effect
scorecard). `scripts/kp-verify.sh` additionally runs the benchmark budget and
the deterministic length policy. The individual commands are still available:

```sh
cargo test
cargo check -p soma-core --target wasm32-unknown-unknown
```

## Provenance

This is a clean-room implementation. See [`PROVENANCE.md`](PROVENANCE.md): no
code from a prior Typst project is included or referenced; public references
are the PRD, the published Knuth-Plass algorithm, and cited CJK rules. No
Typst-dependent code is included.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
