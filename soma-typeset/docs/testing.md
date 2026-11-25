# Testing and gates

This document records the TDD seams and the repository gates. It is part of the
K1 contract: tests are written at public boundaries, not against private
helpers.

## Seams under test

| Seam | Public boundary | What it protects |
|---|---|---|
| Parameters | `soma_core::params::{ParamSet, BreakParams, CostParams, ...}` and `soma_core::preset::default_v1` | Preset values, validation, deterministic hash |
| Breaker behavior | `soma_core::break_paragraph` / `break_paragraph_greedy` | Optimality, tie-break, fallback flags, unsupported align |
| Canonical result | `soma_core::result::LayoutResult` | Schema, field order, byte stability |
| Rules | `soma_rules` public compiler output | Rule status and forbidden/forced boundaries |
| Evaluation | `soma_eval::{bench, sweep_*, blind_report}` | Benchmark metrics, sensitivity, Wilson/kappa decisions |
| Demo | `soma_demo::render_html` | Absolute atom positions, no browser re-breaking |

Tests must not call private functions or assert internal data structures. A
refactor that keeps the public behavior should not break a test.

## TDD loop

1. Pick one seam and one observable behavior.
2. Write a failing test at that seam.
3. Add only enough implementation to make it pass.
4. Run `scripts/gates.sh`.
5. Refactor separately; do not mix refactoring into the red/green step.

The K1 contract tests currently live in:

- `crates/soma-core/tests/k1_contract.rs` — parameter preset, validation, hash.
- `crates/soma-core/tests/k1_breaker.rs` — Justify and Left support, Center and
  Right rejection, final-line penalty zero.

The first K1 slice is green:

- `ParamSet::default_v1`, `validate`, and `param_hash` are implemented and
  covered by `k1_contract.rs`.
- `BreakConfig` now holds a `ParamSet`; layout validates it and carries the
  parameter hash/versions.
- The production breaker accepts Justify and Left and rejects Center/Right with
  `BreakError::UnsupportedAlign`; the layout shifts centred lines after breaking
  (ADR-0031), so `k1_breaker.rs` covers both sides of the contract.
- The cost model uses the frozen normalized `line`, `badness`,
  `break_penalty`, and `short_last` terms; `brute_force_matches_dp` and
  `tie_break_prefers_the_longer_last_line` lock the reference behavior.
- `k1_fixtures.rs` contains 10 public-seam fixtures.
- K2 adds `BoundaryRole`, `Line.boundary_deviation`,
  `Line.cjk_latin_adjustments`, canonical schema `soma.layout/2`, and
  `break_paragraph_traced` (per-line cost breakdown; not canonical).
- K3 adds `GlyphMetrics`, `Line.ascent/descent/leading/height`, inline large
  atoms, baseline shifts, and `height_cost` behind `features.height_cost`;
  canonical schema is now `soma.layout/3`.
- K4 adds edge trimming (`Atom.compress_left/right` → `Line.trim_left/right`),
  the rendering coordinate convention, `ratio_milli`, and compression cost;
  canonical schema is now `soma.layout/4`. The rules layer now emits trim caps
  for opening, closing, and middle punctuation.
- K5 adds `Breakability::Forced`, pre-split segments, `Line.forced_end`, and
  `features.forced_breaks`; canonical schema is now `soma.layout/5`.
- K6 adds result reporting (`breaker_used`, `fallback_stage`,
  `length_limited`, `emergency_lines`, `forbidden_violations`), the
  deterministic `max_kp_atoms` → Greedy-C policy, the conservative atom-count
  window, and the `Normal → Emergency → ForbiddenRescue` ladder; canonical
  schema is now `soma.layout/6`. The rule inventory lives in
  `soma_rules::RULES`.

K4/K5/K6/K8 public tests:

- `crates/soma-core/tests/k4_trim.rs` — target matching, end-before-start
  order, single-atom double trim, fallback when trim is insufficient,
  no trim on underfull lines, compression cost, class independence,
  `features.edge_trim = false`, canonical trim fields, and unsupported
  `trim_priority` rejection.
- `crates/soma-core/tests/k5_forced.rs` — segment split, no repeated indent,
  no stretch/short-score at forced ends, consecutive forced breaks, greedy
  parity, canonical `forcedEnd`, and `features.forced_breaks = false`.
- `crates/soma-core/tests/k6_reporting.rs` — breaker kind, normal/emergency
  stage, `max_kp_atoms` → Greedy-C, `length_limited`, emergency-line count,
  trace flags, and canonical reporting fields.
- `crates/soma-core/tests/k6_fallback.rs` — forbidden rescue as the last
  resort, rescue disabled, normal-stage non-violation, emergency line flags.
- `crates/soma-eval/src/lib.rs` — deterministic corpora, bench rows, sweep
  determinism, Wilson interval, Fleiss kappa, CSV parsing, blind decisions.
- `crates/soma-demo/src/lib.rs` — dual-algorithm HTML, canonical JSON, escaping,
  integer-position rendering.
- `crates/soma-rules/tests/vertical_slice.rs` — trailing comma trims at the
  line end; leading opening bracket trims at the line start; a hard break
  compiled by `paragraph_from_lines` becomes a forced segment end.

Cumulative public test files (counts verified on 2026-11-25 against
`cargo test --workspace -- --list`):

| File | Tests | Covers |
|---|---|---|
| `k1_contract.rs` | 5 | preset, validation, override, hash |
| `k1_breaker.rs` | 4 | Justify and Left accepted, Center/Right rejected, final-line penalty zero |
| `k1_fixtures.rs` | 10 | public-seam fixtures |
| `k2_boundary.rs` | 5 | boundary role, deviation, trace |
| `k3_styles.rs` | 6 | glyph metrics, line boxes, height cost |
| `k4_trim.rs` | 13 | edge trim and coordinates, including justified and ragged line-start opening marks |
| `k5_forced.rs` | 7 | forced breaks and segments |
| `k6_reporting.rs` | 7 | breaker/stage/length flags |
| `k6_fallback.rs` | 4 | emergency and forbidden rescue |
| `a1_canonical.rs` | 10 | DocIR canonical bytes, escaping, hash modes, index markers |
| `a1_model.rs` | 9 | node coverage, renumbering, text projection |
| `a1_raw.rs` | 4 | raw payload preservation |
| `a1_validate.rs` | 26 | every DocIR invariant has a failing case (incl. table grid) |
| `d31_reader.rs` | 10 | canonical JSON reader: rich round-trip, escapes/surrogates, unknown fields, malformed values |
| `d34_date.rs` | 4 | Gregorian month days, leap years, canonical bytes unchanged |
| `d35_traversal.rs` | 2 | shared and mutable meta traversals visit the same nodes |
| `a2_template.rs` | 22 | YAML subset, validation, positions, hash, `within`, `table:`, `masters:`, `toc:`, `start-master`, `level`, `numbering:`, front matter, lists, the Plex reference design |
| `a2_units.rs` | 8 | exact conversion, truncation, strict grammar |
| `a2_resolve.rs` | 12 | cascade, inheritance, page geometry, style hash, heading levels |
| `a2_cell_context.rs` | 1 | `within: cell` overrides body rules and sets cell padding |
| `d42_flow.rs` | 4 | flow sequences/mappings, nesting, malformed input |
| `d43_inherit.rs` | 6 | data-level base/overlay merge, append vs replace, validation |
| `d44_percent.rs` | 7 | exact percent parsing, page margins, line-height, unsupported bases rejected |
| `d47_combinators.rs` | 4 | `parent` and ordered `ancestors` matching, no-match cases |
| `a2_reference_hashes.rs` | 1 | Plex/book reference canonical bytes and hashes unchanged |
| `a3_layout.rs` | 7 | paragraph boxes match the kernel, text index, stacking |
| `a4_pagination.rs` | 13 | vertical K-P optimality, page breaks, keep-with-next, splitting, overflow |
| `a6_blocks.rs` | 31 | code (one atom table, spaces kept), display math box, figures, tables (+colspan, rule separation, natural centring, dash shares), rules, raw, footnotes (rule, number column), marker width, numbered equations, tags and notags, a tag carrying maths, list markers, hanging indent and container indent |
| `metrics.rs` (layout) | 11 | table-backed metrics, per-style scaling, bold face, determinism, outline ink, rescue face, Han inside a formula measured with the text face, inline code measured with its style |
| `e2_split.rs` | 7 | line conservation, widows/orphans, footnote lines, glue, vertical hash |
| `e3_typography.rs` | 2 | hanging punctuation is post-processing and respects the cap |
| `e4_master.rs` | 6 | header/footer bands, folio, running title, margin limits |
| `e5_grid.rs` | 3 | baseline grid alignment, unchanged text, wider grid leading |
| `e7_tables.rs` | 3 | rowspan coverage without overlap, repeated header on every table page |
| `e8_table_split.rs` | 4 | row bands continue across pages, spanning rows stay whole, tall cells are moved not clipped |
| `e9_table_rules.rs` | 8 | outer/header/internal rules, continuation headers, spanning/colspan skips, frame, dashes, color |
| `e10_masters.rs` | 5 | section masters, forced page start, shared content box, unknown names, folio at the bottom of its band |
| `gate_r.rs` (svg) | 4 | Gate V/R: SVG golden, coordinates = layout, HTML/SVG atom parity |
| `table_rules.rs` (svg) | 1 | rules are painted at layout coordinates |
| `table_rules.rs` (html) | 1 | rules reach the HTML renderer as positioned boxes |
| `effect.rs` (eval) | 7 | Gate Q′ structural claims on the scorecard |
| `engine.rs` | 17 | `soma.engine/1`: parse, reject, canonical hash, preset selection, layout equivalence |
| `corpus.rs` (eval) | 10 | `corpus/zh`: presence, determinism, widows/orphans, compression budget, calibration, table/math/book fixtures |
| `a5_md.rs` | 24 |
| `link_reference.rs` | 5 | full/collapsed/image references, label normalization, undefined refs stay text | Markdown subset (nested lists, GFM tables with dash-derived column shares, setext headings), inlines (including GFM `~~`), spans, index/ruby extensions, front matter, validity |
| `mixed.rs` | 12 | Markdown + embedded LaTeX/HTML: inline and display math, currency heuristic, LaTeX commands/environments, semantic inline HTML, HTML blocks spanning blank lines, nested Markdown in HTML, entity escaping, visible inline LaTeX footnote fallback, TikZ/Mermaid environments |
| `html.rs` (html) | 24 | HTML subset shapes incl. definition lists, `ins`, `sup`/`sub`, `hr`, figure/caption, table grid, degradation, recovery, pipeline |
| `latex.rs` (latex) | 18 | LaTeX subset shapes, math source, table/figure captions, `\includegraphics`, metadata, subtitle to cover, inline command opening a block, ruby, recovery, pipeline |
| `macros.rs` (latex) | 14 | source-level expansion: 0–2 argument macros, nesting, renew, depth limit, `\def`, counters (arabic/roman/alph), definitions vanish, comments, verbatim untouched |
| `latex` unit | 8 | inline/block parsing helpers, macro expansion seams |
| `math.rs` (soma-math) | 63 | scripts, fractions, radicals, big operators, fences, matrices, aligned, cases, TeX spacing table, Bin normalisation, style levels, rule 18, `\sin`/`\binom`/`\operatorname`/`\overline`/`\hat`, errors, italic variables, tags |
| `e11_math.rs` (layout) | 10 | centring, baseline inside the box, flow, canonical `math` field, default display skip |
| `e12_inline_math.rs` (layout) | 12 | marker atoms, line-box growth, text style, placeholder, canonical `inlineMath`, splitting a long formula at relations |
| `e27_tikz.rs` (layout) | 4 | a picture is one centred vector box, node labels are items, paths only in canonical when present, source fallback, atomic flow |
| `d56_section_footnotes.rs` (layout) | 5 | section-local footnote numbering, same/next page, repeated refs, canonical numbers |
| `d64_margin_truncation.rs` (layout) | 5 | long headers/running titles truncate with `…`, folio alignment, band containment |
| `d92_body_height.rs` (layout) | 3 | automatic vs explicit body height, page-break choice, vertical hash |
| `tikz.rs` (soma-tikz) | 11 | centimetre unit, shift/scale, circle/arc cubics, named coordinates and anchors, `\foreach` and its tail, grid lattice and step, a filled grid has no area, node measurement, orthogonal paths |
| `e13_toc.rs` (layout) | 13 | TOC depth, page numbers, row shape, leaders, wrapped rows, forced page, figure/table lists, section-start master |
| `e14_alignment.rs` (layout) | 7 | left/center/right placement, first-indent interaction, justify unchanged, centered quote keeps centring |
| `e15_cover.rs` (layout) | 7 | cover metadata, master, centring, fallbacks, unknown master error |
| `e16_numbering.rs` (layout) | 8 | chinese-chapter, arabic-dotted, depth, figure/table counters, numbered TOC entries |
| `e17_front_matter.rs` (layout) | 5 | continuous/roman/hidden labels, `{pages}` = body pages, no folio on hidden pages |
| `e18_captions.rs` (layout) | 4 | figure/table caption elements, indent reset, emphasis keeps the caption base |
| `e19_outline.rs` (layout) | 3 | one bookmark per heading, printed labels, empty document |
| `e20_index.rs` (layout) | 13 | sorted unique terms, page lists and ranges, own page, entry styles, centered `indexTitle`, empty index, sub-entry indentation, orphan parents, pinyin order, `see`, table-cell and footnote markers |
| `e25_ruby.rs` (layout) | 4 | one base atom, annotation text and geometry, line ascent reserved, canonical key only when present |
| `e26_hyphen.rs` (layout) | 5 | pattern breaks, hyphen charged to the line, no patterns keeps the word whole, URLs untouched, canonical marker |
| `e21_line_end_punctuation.rs` (layout) | 6 | trailing bearing trimmed on justified lines, ink on the text edge, ragged lines untouched |
| `e22_fallback.rs` (layout) | 2 | a rescue face measures the glyph and its edge is what a line end trims |
| `e23_space_shrink.rs` (layout) | 2 | a space gives up at most a third of itself; an unbreakable word still lays out |
| `cli.rs` (soma-cli) | 29 | render md/tex to html/svg/json/pdf, template geometry, LaTeX article defaults, TOC template, example book, fonts, automatic math face, mixed Markdown/LaTeX/HTML, `--metrics`, `--out`, stdin `--from`, directory batch, check, error paths |
| `docx.rs` (docx) | 12 | zip store/deflate, headings from style names, `basedOn` styles, tables, lists, metadata, embedded media, damaged archives |
| `blind_pipeline.rs` (eval) | 3 | dry-run fixture through key + statistics + decision |
| `k7_object_metrics.rs` | 4 | object hooks vs text paths, default-hook equivalence |
| `kp_window.rs` (core) | 2 | the conservative candidate window: same breaks as full DP, bounded work |
| `pdf.rs` (render-pdf) | 14 | TrueType cmap/subset, CFF subset and `/CIDFontType0`, name-keyed promotion, `seac` composite, CFF2 refusal, PDF objects, bookmarks, underlines and `/Annots`, image XObject and placeholder fallback, tagged structure tree, byte determinism |
| `math.rs` (svg) | 4 | exact formula coordinates |
| `inline_math.rs` (svg) | 2 | inline items on the line baseline, marker never drawn |
| `images.rs` (svg) | 3 | a resolved figure is a `data:` URI in both renderers, no source path leaks, no assets keeps the placeholder |
| `math.rs` (html) | 4 | formula preview spans, placeholder fallback |
| `inline_math.rs` (html) | 2 | inline preview positions, marker never drawn |
| `a5_render.rs` | 8 | absolute positions, escaping, no re-breaking, page fragments join, block line-height ratio |
| `capabilities.rs` (md) | 10 | Gate C matrix, degradation promise, docs parity, semantic HTML/math entries |
| `safety.rs` (render) | 3 | Gate C: no scripts, links, images, or URLs |
| `pipeline.rs` (md) | 3 | Markdown -> DocIR -> pages -> HTML |
| `spans.rs` (rules) | 6 | span compiler agrees with the text compiler |
| `soma-doc` doc-test | 1 | crate-level usage example |
| `soma-eval` unit | 12 | bench, sweep, Wilson, kappa, blind, kit key translation, `length_limited` share |
| `perturbation.rs` (eval, ignored) | 1 | ±1/4096 advance sensitivity on the corpus; run with `--ignored` |
| `soma-demo` unit | 6 | HTML geometry, escaping, anonymized blind sheet, deterministic sides |
| `soma-wasm` unit | 1 | fixture determinism + forced end + DocIR |
| `vertical_slice.rs` (rules) | 6 | end-to-end rules + trim + hard break |
| `soma-rules` unit | 19 | segmentation, punctuation, half-width bearings, pair trim, hyphen breaks, rule inventory, code-line spaces, dashes keep their width |
| `soma-metrics` unit | 23 | table round-trip, CJK/Latin widths, regular/bold/math faces, hash, truncation, ink sections, v1/v2 compatibility, math-face recognition by recorded advances |
| `soma-core` vertical unit | 1 | vertical cost model hash |
| core unit | 25 | IR, units, breaker, result, presets (`latex-v1` included) |
| layout unit | 19 | trim scaling at body and non-body sizes; math metric adapter; index grammar, ordering and page ranges; column hints |
| `pagination.rs` unit | 4 | a figure and its caption never split; two figures fill one page; debug page composition; body-height page choice |
| `k7_rhythm.rs` (core) | 3 | preset validity and hash, class jumps removed by the rhythm term, zero weight equals the old solver |
| `soma-collate` unit | 4 | pinyin rank lookup, Latin before Han, shared prefixes, table coverage |
| `soma-hyphen` unit | 10 | pattern parsing (`%`, `\patterns{}`), the `hyphenation` fixture, left/right limits, UTF-8 offsets, ordering |
| `soma-docx` unit | 20 | zip store/deflate, inflate, XML entities, heading/style parsing, tables, lists, metadata, embedded media |
| `languages.rs` (highlight) | 12 | Rust/C/C++/shell/Markdown/LaTeX/Python/JS/TS/JSON/YAML/TOML/HTML/CSS/SQL and friends: class spans per line |
| `abi.rs` (web) | 8 | the C ABI slice: compile/measure/break handles, parity with the native kernel, fallback policy |
| `soma-md` unit | 5 | frontend-internal projection helper; embedded HTML tag/attribute scanner; unquoted self-closing value; nested close matching; LaTeX command argument scanner |
| `render-pdf` unit | 28 | CFF INDEX round trip, compact glyph-preserving subset with a CID charset, name-keyed promotion, `seac` expansion placed point by point, decoration geometry and the PDF y-flip, link merging, eight deflate round trips against an independent decoder (fixed and dynamic blocks) plus a depth-limit/Kraft check, inflate against our own encoder and a stored block, PNG decode, JPEG header parse, alpha compositing, TikZ path fill state |
| `studio.rs` (studio-core) | 7 | fragments follow pages and join the full document, probe samples come from layout faces, build determinism, hyphenation patterns reach the shared build, a Mermaid fence becomes vector paths, the same diagram exports to TikZ |
| `soma-studio-ui` unit | 2 | assistant markdown escapes raw HTML and rejects `javascript:`/`data:` destinations |
| `soma-studio` unit | 5 | origin allowlist: loopback default, concrete LAN host, wildcard `Host` matching, IPv6 literals, `--host` parsing |
| `mermaid.rs` (diagram) | 6 | Mermaid and DOT layouts, paths/labels, TikZ round trip, detection rules, directions/shapes/classes, clusters and self-loops |

Total: **977 tests passed** (`cargo test --workspace`), plus **1 ignored**
(`perturbation.rs`, run with `--ignored`). Counting that ignored test, the
workspace lists **978 test functions**. The standalone
`tools/metrics-gen` crate adds its own 5 (`cargo test --manifest-path tools/metrics-gen/Cargo.toml`).

Still deferred: the human blind-test run (D01), a quality-labeled corpus
(D67), italic faces/tracking/kerning (D61/D60/D100), nested box trees and
per-master content geometry / page-level search (D51/D62/D66), and the TikZ
library surface (D99).

## Repository gates

Run:

```sh
scripts/gates.sh
```

The script runs:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo check -p soma-core -p soma-doc -p soma-engine -p soma-metrics -p soma-highlight -p soma-template -p soma-layout -p soma-tikz -p soma-diagram -p soma-md -p soma-render-html -p soma-render-svg -p soma-render-pdf --target wasm32-unknown-unknown`
5. native vs `wasm32-wasip1` canonical-byte comparison (`scripts/wasm-hash.sh`)
6. a demo smoke render (`soma-demo` → temporary HTML, check `data-algorithm`)
7. `cargo tree -p soma-core` dependency-boundary check
8. clean-room reference check: active files must not mention the deleted prior project
9. Gate A (`scripts/gate-a.sh`): layer dependencies, `soma-doc` dependency-free and
   `no_std`, dependency whitelists, no legacy semantic exports, no floats, schema
   constants, no physical style fields in DocIR, no engine knobs in templates
10. Gate C (`scripts/gate-c.sh`): capability matrix parity, degradation promise,
    renderer safety
11. Gate V/R (`scripts/gate-r.sh`): SVG visual lock and cross-renderer atom parity
12. Gate Q′ (`scripts/gate-q.sh`): effect scorecard and real-corpus baseline
13. Gate S (`scripts/studio.sh`, separate): S1–S11 for the Studio — shell
    baselines, tabs, panels, themes, page fragments and Shadow DOM, live
    rebuilds and IME, inspect/overlay agreement, CSP and external-resource
    checks, font probe with/without fonts, SSE `built`/`error`, PDF page count,
    and the assistant's no-engine-write boundary

CI calls the same script, so a local pass and CI pass mean the same thing.

## Gate T / Q / D mapping

| Gate | What it checks | Blocks |
|---|---|---|
| Gate T | Technical tests and invariants: brute force, `K-P ≤ Greedy-C`, canonical bytes, params, fixtures, wasm | K1–K8 |
| Gate A | Architecture: layer dependencies, DocIR purity, schema constants, `no_std`, no floats | A1–A6 |
| Gate C | Content: capability matrix, degradation keeps text, renderer emits no active content | A5+ |
| Gate Q | Quality claim: guard/target metrics, pre-experiment κ, 30×5 blind test, CI; may be Inconclusive | Only the public quality claim |
| Gate D | Delivery: WASM size/latency (`scripts/web.sh` W1–W6, ADR-0071), demo, reproducible scripts, provenance | Release |
| Gate S | Studio: shell/tabs/panels/theme, wasm/native hash equality, PDF page count, SSE reloads, font probe, CSP, assistant boundary | Web Studio |

`Gate T` is the only gate that can block engineering progress. `Gate Q` must
never be silently reported as “passed” when it is `Inconclusive`.
