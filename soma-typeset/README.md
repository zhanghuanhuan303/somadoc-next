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
- A3–A6 pipeline: `soma-layout` builds paragraph boxes over the frozen kernel
  and paginates greedily (`soma.pages/1`), with footnote areas, figure boxes,
  table cell boxes, code/math/rule boxes, and a `TextSource` seam;
  `soma-md` parses a documented Markdown subset into DocIR; `soma-render-html`
  paints absolutely positioned HTML. The WASI fixture now hashes DocIR, inline
  layout, and pages together.
- A2 template layer (`soma-template`, schemas `soma.template/1` and
  `soma.styles/1`): a documented YAML subset with line/column errors, exact units
  (decimal → rational → 1/4096 em, truncation toward zero), a selector cascade
  with inheritance, a 16-property whitelist, `templateHash`, and `StyledDoc` as
  lifetime-free data with `styleHash`. Templates cannot write engine knobs; Gate A
  enforces that statically.

## What is not here yet

- Templates (`soma-template`), the Layout Box Tree (`soma-layout`), pagination,
  Markdown/HTML/LaTeX front ends, the real HTML renderer, footnote/figure/table
  layout, browser WASM packaging, real font metrics, incremental layout, and the
  actual human blind-test run. Gate Q remains `Inconclusive` until that run
  exists. See [`docs/roadmap-v2.md`](docs/roadmap-v2.md) and
  [`docs/debt.md`](docs/debt.md).

## Limitations of this first slice

- `SyntheticMetrics` is a deterministic test fake, not a font table. The real
  metrics table generator comes next.
- The punctuation hard rules are a deliberately small set, now cited against
  GB/T 15834-2011 via W3C clreq §6.1.1 (basic). Trim caps are present and
  active, but their ratios remain `ParamOnly` and uncalibrated.
- There is no hyphenation: Latin and digit runs are unbreakable atoms.
- Abbreviations with periods and URLs can split at their non-word punctuation;
  the sample set should avoid treating them as words until the rule layer grows.
- Edge trimming is applied only at line edges and only when a line is overfull;
  hanging punctuation (negative trim) is not implemented.
- Forced breaks must currently be emitted by the caller or through
  `paragraph_from_lines`; Markdown hard-break syntax is not parsed yet.
- The demo uses `SyntheticMetrics` and browser fonts; it is a geometry proof,
  not a font-accurate specimen.

## Layout

```text
crates/
  soma-core/   # frozen inline kernel: units, ParaIR, metrics, breaker, result
  soma-doc/    # DocIR: nodes, source spans, validation, canonical JSON, hashes
  soma-template/ # pure-YAML template: parse, exact units, cascade, StyledDoc
  soma-layout/ # paragraph boxes, pagination, footnotes, PagedLayout
  soma-md/     # Markdown subset -> DocIR
  soma-render-html/ # PagedLayout -> absolutely positioned HTML
  soma-rules/  # text -> ParaIR (Chinese punctuation + CJK/Latin segmentation)
  soma-eval/   # benchmark, sensitivity sweeps, blind-test statistics
  soma-demo/   # static HTML renderer consuming LayoutResult
  soma-wasm/   # native/WASI fixture for cross-target byte verification
scripts/
  gates.sh     # fmt, clippy, tests, wasm hash, demo smoke, dependency guard, Gate A
  gate-a.sh    # architecture gate: layer boundaries, DocIR purity, schema versions
  bench.sh     # P50/P95 budget check
  sweep.sh     # tolerance x badness sweep
  demo.sh      # target/demo.html
  wasm-hash.sh # native vs wasm32-wasip1 canonical bytes
  kp-verify.sh # K-P closure: gates + bench budget + length policy
docs/
  architecture-v2.md  # frozen architecture
  adr/                # accepted decisions (ADR-0001 … 0009)
  roadmap-v2.md       # A1–A6 schedule, work packages, gates
  debt.md             # global not-done register (D01–D37, N1–N6)
  docir.md            # DocIR contract: nodes, canonical JSON, validation
  template.md         # template DSL: YAML subset, units, cascade, hashes
  a1-report.md        # A1 milestone report (Gate A1 checklist)
  a2-report.md        # A2 milestone report (Gate A2 checklist)
  a3-a6-report.md     # A3–A6 report: layout, pagination, Markdown, HTML, footnotes
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

The gate runs format, clippy, 237 tests, a `wasm32-unknown-unknown` check for
`soma-core`, `soma-doc`, `soma-template`, `soma-layout`, `soma-md`, and
`soma-render-html`, a native/`wasm32-wasip1` canonical-byte comparison (DocIR +
inline layout + pages), a demo smoke render, the `soma-core`
dependency-boundary check, and Gate A. `scripts/kp-verify.sh` additionally runs
the benchmark budget and the deterministic length policy. The individual
commands are still available:

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
