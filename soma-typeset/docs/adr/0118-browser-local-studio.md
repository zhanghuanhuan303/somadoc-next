# ADR-0118: Browser-local Studio on the public host

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0109 (SVG proof), ADR-0117 (reverse-proxy origins),
  `docs/proposal-wasm-cdn.md` (delivery)
- Touches: `soma-studio-ui` (`local`, `engine`, `state`, `verify`, `selftest`),
  `soma-studio` static cache headers, `scripts/local-assets.sh`

## Context

The Studio already parses, breaks, paginates and renders the SVG proof in
wasm. A remote deployment still called the Axum server on the editing path:
every successful rebuild triggered `POST /api/verify` (the native comparison),
and PDF/TikZ/save went through `/api`. The font API answered with
`Cache-Control: no-store`, so a cold page re-downloaded 19 MB of CJK faces.

On a high-latency origin the wasm build (about 20 ms) was hidden behind those
round-trips. The engine itself was not slow; the remaining server calls were.

## Decision

1. **Local mode is the default on `somadoc.art` and `*.somadoc.art`.** A page
   can force the old behaviour with `?server=1`, and any other host can opt in
   with `?local=1`. Loopback and LAN hosts stay on the server path so the S
   gates and the file-tree workflow keep exercising Axum.
2. **The default project is embedded.** `full.tex`, the Plex template, the
   `proposal-v1` engine file, the `.smt` metrics table and the hyphenation
   patterns are compiled into the wasm. No `/api/project`, `/api/metrics` or
   `/api/hyphenation` is needed for the first build.
3. **Verification is local.** `verify_now` derives a `NativeSummary` from the
   same `Build` the proof uses. The native comparison remains available through
   the server mode and the S gates; normal editing no longer posts the source.
4. **PDF and TikZ are local.** `soma-render-pdf` is linked into the UI wasm.
   The exporter builds `PdfFonts` from full faces fetched once from
   `/fonts/v1/`, uses the image bytes already loaded for the proof, and
   downloads the bytes with a Blob. TikZ uses the core `diagram_tikz` function.
5. **Fonts are static and split by use.** The proof uses subset WOFF2 files
   (about 0.5 MB total for the demo plus the Studio chrome). PDF export fetches
   the complete TTF/OTF faces (about 19 MB) on first use; the browser cache then
   serves them. `/fonts/v1/` is immutable and versioned.
6. **Static cache policy.** The server sends `immutable` for `/fonts/`, a
   one-hour TTL for `.wasm`/`.js`/`.css` (until Trunk hashes names), and leaves
   HTML revalidating. The API keeps `no-store`.
7. Save in local mode downloads the current tab; there is no server write.

## Consequences

- Typing, verification, TikZ and PDF no longer wait for a network round-trip.
  The S8 invariant (no external resources) still holds: fonts, figures and the
  wasm are same-origin static files.
- First paint is about 4 MB of wasm (about 1 MB brotli/gzip in transit) plus
  about 0.5 MB of subset fonts. The first PDF export additionally fetches the
  full faces; this is explicit and cached.
- Local mode exposes one embedded document. Multi-file open/save through the
  browser File System Access API is not implemented yet; `?server=1` retains
  the server file tree.
- No change to `DocIR`, layout, canonical bytes, `modelVersion` or
  `rulesVersion`. The native/wasm layout hash is unchanged.
- The S gates still run server mode on loopback. The local S12 check renders a
  PDF entirely in wasm and asserts the `%PDF-` header.

## Alternatives considered

- **Only remove `/api/verify`.** Rejected as the whole fix: PDF and save still
  crossed the network, and the no-store font endpoint kept reloading 19 MB.
- **Keep PDF on the server.** Rejected for the public host: the user asked for
  all engine work to be local, and `soma-render-pdf` is already `no_std` and
  wasm-compatible.
- **Use system fonts for the proof.** Rejected: the proof would no longer match
  the measured Plex metrics, and the font probe would report a false mismatch.
- **Ship full CJK faces on first paint.** Rejected: 19 MB defeats the latency
  goal; subset WOFF2 keeps the proof exact for the document while PDF keeps
  full coverage.
- **Make every host local by default.** Rejected: the NAS and loopback
  workflows rely on the server file tree and native verification.
