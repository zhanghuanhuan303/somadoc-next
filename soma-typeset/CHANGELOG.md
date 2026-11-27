# Changelog

## Unreleased — 2026-11-28

- List items hang by the measured marker prefix (`• `, `1. `, `10. `), not by
  the template's `first-indent` guess. The text after the marker is pinned to
  the wrapped-line column and the marker is kept on its hang column, so a
  wrapped item and an unwrapped item share both columns exactly; `modelVersion`
  5 (ADR-0123).
- The Studio source pane now shows a diagnostics panel above the editor:
  line/column, message and a suggested fix. Exact spans get a GitHub-style red
  wavy underline through a transparent text mirror; clicking a row selects the
  span (ADR-0122).
- Source lints cover unclosed fences, math parse errors, undefined/duplicate
  footnotes, unclosed `[[...]]` extensions and unmatched LaTeX environments;
  build errors map to a node span when the message names `node N`.
- S14 locks the panel, wavy spans, suggestions and click-to-select in the
  static gate.

## 2026-11-27

- The public Studio is a pure static bundle (ADR-0121). Apache serves `dist/`
  and `/fonts/v2/`; there is no `/api` and no Axum process on somadoc.art.
- `is_local()` now defaults to local on every non-loopback host, so a CDN
  preview or custom domain cannot silently call `/api`.
- Static mode opens `.md`, `.html`, `.tex` and `.docx` in the browser via the
  file picker or a window drop; text and DOCX bytes never leave the page.
- Local verification shows the wasm build summary. The chrome no longer claims
  a runtime `wasm = native`; cross-target equality remains a CI gate.
- `scripts/static.sh` serves `dist/` without Axum and locks S1/S3–S10/S12/S13,
  zero `/api` requests, and a 404 for `/api/health`.

## 0.1.0 — 2026-11-26

The first open-source snapshot.

- Deterministic, integer-only typesetting core: DocIR, pure-YAML templates,
  Knuth–Plass line breaking, vertical pagination, footnotes, tables, math,
  diagrams, indexes, PDF/HTML/SVG renderers.
- Four frontends: Markdown (with embedded LaTeX/HTML), HTML, LaTeX and DOCX.
- All-Rust Studio: the whole engine compiled to wasm, a browser-local mode with
  local verification, TikZ and PDF, and an optional Axum path for files and
  native comparison.
- GB2312 levels 1+2 font subsets shared by the proof and PDF exporter; no
  coverage check and no full-font fallback in the hosted demo.
- Content-hashed UI assets, versioned `/fonts/v2/`, and a single
  `welcome.md` entry document at <https://somadoc.art>.
- 1,006 tests, 120 ADRs, 28 crates; the dated map is
  [`docs/soma-atlas.html`](docs/soma-atlas.html).
