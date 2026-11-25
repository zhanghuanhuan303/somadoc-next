# ADR-0109: The Studio proof is SVG rendered from `PagedLayout`

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0093 (all-Rust wasm Studio), ADR-0049 (SVG coordinates),
  ADR-0108, `docs/proposal-svg-proof.md`
- Touches: `soma-render-svg` (fragments, font attributes), `soma-studio-core`
  (`Build::svg_fragments`), `soma-studio-ui` (`engine.rs`, `proof.rs`,
  `selftest.rs`, `studio.css`), `docs/studio.md`

## Context

The Studio proof was HTML emitted from the same `PagedLayout` as the PDF, but
the browser then laid the text out again. Font fallback, line-height rounding
and math positioning could therefore differ from the PDF; the user saw maths
"drift" on screen while the downloaded PDF was correct.

The proposal `docs/proposal-svg-proof.md` argued for making the preview a
renderer of the same coordinates, with PDF only on download. This ADR records
the implementation.

## Decision

1. `soma-render-svg` exposes `render_fragments` / `render_fragments_with`.
   Each fragment is one `<svg class="page" data-page="N" width="100%"
   height="100%" viewBox="0 0 W H">` whose coordinates are layout units.
2. Every block is a `<g class="block" data-node data-family>`; text carries
   `data-node`, `data-atom`, `data-family`, generic `font-family`, and
   `font-weight`/`font-style` when the style requests them. Mathematics keeps
   `data-math`.
3. `soma-studio-core::Build::svg_fragments` renders the proof with the same
   image source as the HTML/PDF paths; images are inlined data URIs.
4. The Studio proof mounts SVG fragments, not HTML fragments. The HTML
   renderer remains for HTML export and its tests.
5. The Studio Shadow CSS maps `data-family`/`data-math` to the session's
   `@font-face` families (`--soma-serif`, `--soma-sans`, `--soma-mono`,
   `--soma-math`), so the SVG names the same faces as the PDF metrics.
6. Inspection overlays stay in `PagedLayout` coordinates; they never read SVG
   DOM boxes. S7 still selects `.ov-line` elements.
7. PDF is still generated only by `POST /api/pdf` on download.

## Consequences

- Preview and PDF share page count, block/line count and atom coordinates;
  the browser no longer reflows the page.
- SVG text remains selectable and searchable; no glyph outlines are embedded.
- A missing font can still change the shape, but not the atom positions;
  S9 reports the installed family.
- The wasm binary grows by the `soma-render-svg` code (about 5 KB before gzip
  in the measured build). The proof is generated in wasm; native and wasm
  hashes remain equal.
- S5 now counts `svg_fragments`; S8 still forbids scripts and external links;
  S11 reads `textContent` from the SVG page.
- `docs/proposal-svg-proof.md` moves to implemented status; PDF-on-download was
  already the server behavior and is unchanged.
