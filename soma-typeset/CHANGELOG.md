# Changelog

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
