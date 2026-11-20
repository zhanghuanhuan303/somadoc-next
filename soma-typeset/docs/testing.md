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

Cumulative public test files:

| File | Tests | Covers |
|---|---|---|
| `k1_contract.rs` | 5 | preset, validation, override, hash |
| `k1_breaker.rs` | 4 | Justify and Left accepted, Center/Right rejected, final-line penalty zero |
| `k1_fixtures.rs` | 10 | public-seam fixtures |
| `k2_boundary.rs` | 5 | boundary role, deviation, trace |
| `k3_styles.rs` | 6 | glyph metrics, line boxes, height cost |
| `k4_trim.rs` | 10 | edge trim and coordinates |
| `k5_forced.rs` | 7 | forced breaks and segments |
| `k6_reporting.rs` | 7 | breaker/stage/length flags |
| `k6_fallback.rs` | 4 | emergency and forbidden rescue |
| `a1_canonical.rs` | 10 | DocIR canonical bytes, escaping, hash modes, index markers |
| `a1_model.rs` | 9 | node coverage, renumbering, text projection |
| `a1_raw.rs` | 4 | raw payload preservation |
| `a1_validate.rs` | 26 | every DocIR invariant has a failing case (incl. table grid) |
| `a2_template.rs` | 20 | YAML subset, validation, positions, hash, `within`, `table:`, `masters:`, `toc:`, `start-master`, `level`, `numbering:`, front matter, lists |
| `a2_units.rs` | 7 | exact conversion, truncation, strict grammar |
| `a2_resolve.rs` | 12 | cascade, inheritance, page geometry, style hash, heading levels |
| `a2_cell_context.rs` | 1 | `within: cell` overrides body rules and sets cell padding |
| `a3_layout.rs` | 7 | paragraph boxes match the kernel, text index, stacking |
| `a4_pagination.rs` | 13 | vertical K-P optimality, page breaks, keep-with-next, splitting, overflow |
| `a6_blocks.rs` | 13 | code, display math box, figures, tables (+colspan), rules, raw, footnotes, marker width |
| `metrics.rs` (layout) | 9 | table-backed metrics, per-style scaling, bold face, determinism, outline ink, rescue face |
| `e2_split.rs` | 7 | line conservation, widows/orphans, footnote lines, glue, vertical hash |
| `e3_typography.rs` | 2 | hanging punctuation is post-processing and respects the cap |
| `e4_master.rs` | 6 | header/footer bands, folio, running title, margin limits |
| `e5_grid.rs` | 3 | baseline grid alignment, unchanged text, wider grid leading |
| `e7_tables.rs` | 2 | rowspan coverage without overlap, repeated header on every table page |
| `e8_table_split.rs` | 4 | row bands continue across pages, spanning rows stay whole, tall cells are moved not clipped |
| `e9_table_rules.rs` | 8 | outer/header/internal rules, continuation headers, spanning/colspan skips, frame, dashes, color |
| `e10_masters.rs` | 4 | section masters, forced page start, shared content box, unknown names |
| `gate_r.rs` (svg) | 4 | Gate V/R: SVG golden, coordinates = layout, HTML/SVG atom parity |
| `table_rules.rs` (svg) | 1 | rules are painted at layout coordinates |
| `table_rules.rs` (html) | 1 | rules reach the HTML renderer as positioned boxes |
| `effect.rs` (eval) | 7 | Gate Q′ structural claims on the scorecard |
| `engine.rs` | 15 | `soma.engine/1`: parse, reject, canonical hash, layout equivalence |
| `corpus.rs` (eval) | 9 | `corpus/zh`: presence, determinism, widows/orphans, compression budget, calibration, table/math/book fixtures |
| `a5_md.rs` | 14 | Markdown subset, inlines, spans, index extension, front matter, validity |
| `html.rs` (html) | 9 | HTML subset shapes, table grid, degradation, recovery, pipeline |
| `latex.rs` (latex) | 15 | LaTeX subset shapes, math source, table/figure captions, `\includegraphics`, metadata, subtitle to cover, recovery, pipeline |
| `math.rs` (soma-math) | 26 | scripts, fractions, radicals, big operators, fences, matrices, aligned, cases, spacing, styles, errors |
| `e11_math.rs` (layout) | 8 | centring, baseline inside the box, flow, canonical `math` field |
| `e12_inline_math.rs` (layout) | 11 | marker atoms, line-box growth, text style, placeholder, canonical `inlineMath` |
| `e13_toc.rs` (layout) | 13 | TOC depth, page numbers, row shape, leaders, wrapped rows, forced page, figure/table lists, section-start master |
| `e14_alignment.rs` (layout) | 6 | left/center/right placement, first-indent interaction, justify unchanged |
| `e15_cover.rs` (layout) | 7 | cover metadata, master, centring, fallbacks, unknown master error |
| `e16_numbering.rs` (layout) | 8 | chinese-chapter, arabic-dotted, depth, figure/table counters, numbered TOC entries |
| `e17_front_matter.rs` (layout) | 5 | continuous/roman/hidden labels, `{pages}` = body pages, no folio on hidden pages |
| `e18_captions.rs` (layout) | 4 | figure/table caption elements, indent reset, emphasis keeps the caption base |
| `e19_outline.rs` (layout) | 3 | one bookmark per heading, printed labels, empty document |
| `e20_index.rs` (layout) | 5 | sorted unique terms, page lists, own page, entry styles, empty index |
| `e21_line_end_punctuation.rs` (layout) | 6 | trailing bearing trimmed on justified lines, ink on the text edge, ragged lines untouched |
| `e22_fallback.rs` (layout) | 2 | a rescue face measures the glyph and its edge is what a line end trims |
| `cli.rs` (soma-cli) | 17 | render md/tex to html/svg/json/pdf, template geometry, TOC template, example book, fonts, `--metrics`, `--out`, check, error paths |
| `blind_pipeline.rs` (eval) | 3 | dry-run fixture through key + statistics + decision |
| `k7_object_metrics.rs` | 4 | object hooks vs text paths, default-hook equivalence |
| `pdf.rs` (render-pdf) | 8 | TrueType cmap/subset, CID-keyed CFF subset and `/CIDFontType0`, name-keyed CFF refusal, CFF2 refusal, PDF objects, bookmarks, byte determinism |
| `math.rs` (svg) | 4 | exact formula coordinates |
| `inline_math.rs` (svg) | 2 | inline items on the line baseline, marker never drawn |
| `math.rs` (html) | 4 | formula preview spans, placeholder fallback |
| `inline_math.rs` (html) | 2 | inline preview positions, marker never drawn |
| `a5_render.rs` | 6 | absolute positions, escaping, no re-breaking |
| `capabilities.rs` (md) | 7 | Gate C matrix, degradation promise, docs parity |
| `safety.rs` (render) | 3 | Gate C: no scripts, links, images, or URLs |
| `pipeline.rs` (md) | 3 | Markdown -> DocIR -> pages -> HTML |
| `spans.rs` (rules) | 6 | span compiler agrees with the text compiler |
| `soma-doc` doc-test | 1 | crate-level usage example |
| `soma-eval` unit | 11 | bench, sweep, Wilson, kappa, blind, kit key translation, `length_limited` share |
| `perturbation.rs` (eval, ignored) | 1 | ±1/4096 advance sensitivity on the corpus; run with `--ignored` |
| `soma-demo` unit | 6 | HTML geometry, escaping, anonymized blind sheet, deterministic sides |
| `soma-wasm` unit | 1 | fixture determinism + forced end + DocIR |
| `vertical_slice.rs` (rules) | 6 | end-to-end rules + trim + hard break |
| `soma-rules` unit | 17 | segmentation, punctuation, half-width bearings, pair trim, hyphen breaks, rule inventory |
| `soma-metrics` unit | 13 | table round-trip, CJK/Latin widths, regular/bold/math faces, hash, truncation, ink sections, v1/v2 compatibility |
| `soma-core` vertical unit | 1 | vertical cost model hash |
| core unit | 24 | IR, units, breaker, result, presets |
| layout unit | 5 | trim scaling at body and non-body sizes; math metric adapter |
| render-pdf unit | 3 | CFF INDEX round trip, glyph-preserving subset with identity charset, name-keyed refusal |

Total: **561 tests**.

Still deferred: a quality-labeled calibration corpus, the human blind-test run,
kerning pairs, italic faces, tracking fallback, and calibrated trim ratios.

## Repository gates

Run:

```sh
scripts/gates.sh
```

The script runs:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo check -p soma-core -p soma-doc -p soma-template -p soma-layout -p soma-md -p soma-render-html -p soma-render-pdf --target wasm32-unknown-unknown`
5. native vs `wasm32-wasip1` canonical-byte comparison (`scripts/wasm-hash.sh`);
   the fixture now contains both `soma.doc/2` and `soma.layout/6` output
6. a demo smoke render (`soma-demo` → temporary HTML, check `data-algorithm`)
7. `cargo tree -p soma-core` dependency-boundary check
8. clean-room reference check: active files must not mention the deleted prior project
9. Gate C (`scripts/gate-c.sh`): capability matrix parity, degradation promise,
   renderer safety
10. Gate A (`scripts/gate-a.sh`): layer dependencies, `soma-doc` dependency-free and
   `no_std`, `soma-template` dependency whitelist, no legacy semantic exports, no
   floats, schema constants, no physical style fields in DocIR, no engine knobs in
   templates

CI calls the same script, so a local pass and CI pass mean the same thing.

## Gate T / Q / D mapping

| Gate | What it checks | Blocks |
|---|---|---|
| Gate T | Technical tests and invariants: brute force, `K-P ≤ Greedy-C`, canonical bytes, params, fixtures, wasm | K1–K8 |
| Gate A | Architecture: layer dependencies, DocIR purity, schema constants, `no_std`, no floats | A1–A6 |
| Gate C | Content: capability matrix, degradation keeps text, renderer emits no active content | A5+ |
| Gate Q | Quality claim: guard/target metrics, pre-experiment κ, 30×5 blind test, CI; may be Inconclusive | Only the public quality claim |
| Gate D | Delivery: WASM size/latency, demo, reproducible scripts, provenance | Release |

`Gate T` is the only gate that can block engineering progress. `Gate Q` must
never be silently reported as “passed” when it is `Inconclusive`.
