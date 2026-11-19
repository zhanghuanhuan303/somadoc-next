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
- A1 DocIR (`soma-doc`, schema `soma.doc/1`): 13 block and 16 inline kinds with
  `NodeId`/`SourceSpan`/`Attrs`, footnotes, figures, tables, math source, raw
  payload preservation, structural validation, canonical JSON, and two hashes
  (`docHash` for exact reproduction, `contentHash` for content identity). The
  crate has **zero dependencies** and no floats; Gate A enforces that.
- A3–A6 pipeline: `soma-layout` builds paragraph boxes over the frozen kernel and
  paginates with **line-level vertical K-P** by default (`soma.pages/6`):
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
- E1–Q′ effect layer: `soma-metrics` ships a committed `soma.metrics/1` table
  generated from Noto Serif/Sans CJK SC (numbers only), `StyledMetrics` scales
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
- Display math (`soma-math`, ADR-0026): a TeX subset (symbols, scripts,
  fractions, radicals, `\sum`/`\int` with limits, function names,
  `\left…\right`) laid out into integer glyphs and rules. `soma-layout`
  centres one `MathBox` per `MathBlock`; both renderers paint it; a malformed
  formula falls back to text instead of failing the page. Inline formulas are
  object atoms measured by their own box (ADR-0027), so they break and raise the
  line box like any other inline object. Fences stretch to their body, and
  `matrix`/`pmatrix`/`bmatrix`/`vmatrix`/`cases`/`aligned` are supported; symbols
  come from a dedicated math face in the metrics table (ADR-0030). See
  [`docs/math.md`](docs/math.md).
- Engine configuration: `soma-engine` parses `soma.engine/1` (preset, window,
  atom window, fallback switches, vertical cost model) into the same
  `BreakConfig`/`VerticalParams` the hand-built path uses. See
  [`docs/engine.md`](docs/engine.md) and ADR-0016.
- Contents pages and chapter openings (ADR-0028): `toc: {depth, title}` in the
  template produces an automatically paginated contents page (page numbers are
  a fixed point over at most four layout passes, reported as `toc.stable`), and
  `page.start-master` selects a master for pages that open a section.
- One command (ADR-0029): `soma render --input doc.tex --template book.yaml
  --format html|svg|json` runs the whole pipeline; `soma check` prints layout
  facts and the canonical hash. See [`docs/cli.md`](docs/cli.md).
- First-release artifact (`examples/book`, ADR-0031): `scripts/book.sh` renders a
  nine-page book from `examples/book/book.tex` + `book.yaml` — generated cover,
  contents page, chapter openings without running heads, a formula, a three-line
  table, and a footnote. Alignment (`left`/`center`/`right`) is a layout pass, so
  titles and cover lines can be centred. The build has 0 overflow pages and 0
  degraded lines; `docs/first-release.md` lists the remaining design gaps.
- Blind-test tooling: kit generation, key unmapping, Wilson interval, Fleiss'
  kappa, and a committable Markdown report (`soma-eval blind --out`). A
  **simulated** dry run proves the chain (`docs/blind-dry-run.md`); the human
  panel (D01) is still open.
- A2 template layer (`soma-template`, schemas `soma.template/6` and
  `soma.styles/3`): a documented YAML subset with line/column errors, exact units
  (decimal → rational → 1/4096 em, truncation toward zero), a selector cascade
  with inheritance, a 25-property whitelist, `templateHash`, and `StyledDoc` as
  lifetime-free data with `styleHash`. Templates cannot write engine knobs; Gate A
  enforces that statically.

## What is not here yet

- Word import, browser WASM packaging, incremental layout, and the human
  blind-test run. Gate Q remains `Inconclusive` until that run exists; the
  effect scorecard (Q′) is a structural report, not a quality claim. See
  [`docs/roadmap-v2.md`](docs/roadmap-v2.md) and
  [`docs/debt.md`](docs/debt.md).

## Limitations

- Font metrics come from a committed table, not from shaping: no kerning pairs
  yet, bold uses regular advances, and rare codepoints fall back to 1em (D61,
  D63).
- No hyphenation dictionary. Latin runs may break after `-` and `/`, and nothing
  else; a long unbreakable run can still compress past its shrink limit (D09,
  D60).
- The punctuation hard rules are cited against GB/T 15834-2011 via W3C clreq
  §6.1.1 (basic). Trim and pair-compression ratios remain `ParamOnly` and
  uncalibrated.
- Page masters are one set per document: no first-page/cover variant, no
  per-section masters (D62). Header/footer text is a single centered line (D64).
- The demo uses the real metrics table, but its HTML is still a geometry proof:
  it paints positions and lets the browser choose the font.

## Layout

```text
crates/
  soma-core/   # frozen inline kernel: units, ParaIR, metrics, breaker, result
  soma-doc/    # DocIR: nodes, source spans, validation, canonical JSON, hashes
  soma-engine/ # soma.engine/1: preset, window, fallback, vertical knobs
  soma-metrics/ # soma.metrics/1 table parser + embedded Noto CJK metrics
  soma-template/ # pure-YAML template: parse, exact units, cascade, StyledDoc
  soma-layout/ # paragraph boxes, line-level pagination, page masters
  soma-md/     # Markdown subset -> DocIR
  soma-html/   # HTML subset (semantic whitelist) -> DocIR
  soma-latex/  # LaTeX subset (sections, inline, environments) -> DocIR
  soma-cli/    # `soma`: source + template -> HTML/SVG/canonical JSON
  soma-math/   # TeX subset -> integer math boxes (symbols + rules)
  soma-render-html/ # PagedLayout -> absolutely positioned HTML
  soma-render-svg/  # PagedLayout -> SVG (Gate V/R)
  soma-rules/  # text -> ParaIR (Chinese punctuation + CJK/Latin segmentation)
  soma-eval/   # benchmark, sweeps, blind-test statistics, effect scorecard
  soma-demo/   # static HTML renderer consuming LayoutResult
  soma-wasm/   # native/WASI fixture for cross-target byte verification
tools/dry-run-panel/ # simulated blind-test panel generator (not human data)
tools/
  metrics-gen/ # offline OTF/TTC -> soma.metrics/1 (numbers only)
examples/
  book/        # first-release book: book.tex + book.yaml (scripts/book.sh)
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
docs/
  architecture-v2.md  # frozen architecture
  adr/                # accepted decisions (ADR-0001 … 0031)
  roadmap-v2.md       # A1–A6 + E1–E7 + Q′ schedule, work packages, gates
  debt.md             # global not-done register (D01–D77, N1–N6)
  proposal-effect-max.md # E1–E6 + Q′ proposal
  e1-q-report.md      # E1–Q′ delivery report
  engine.md           # soma.engine/1: shape, defaults, errors, usage
  corpus-calibration.md # real corpus: metrics and proxy calibration
  docir.md            # DocIR contract: nodes, canonical JSON, validation
  math.md             # display math: TeX subset, box model, style constants
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
scripts/wasm-hash.sh       # native vs wasm32-wasip1 canonical bytes
scripts/bench.sh           # 100..4,000 atoms, P50/P95, budget check
scripts/sweep.sh           # tolerance x badness scale
cargo run -p soma-eval --release -- inspect --kind mixed --atoms 500
cargo run -p soma-eval --release -- blind preferences.csv
```

## Test

```sh
scripts/gates.sh
```

The gate runs format, clippy, 330 tests, a `wasm32-unknown-unknown` check for
`soma-core`, `soma-doc`, `soma-engine`, `soma-metrics`, `soma-template`,
`soma-layout`, `soma-md`, `soma-render-html`, and `soma-render-svg`, a
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
