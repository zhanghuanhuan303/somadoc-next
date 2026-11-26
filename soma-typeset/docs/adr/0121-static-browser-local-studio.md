# ADR-0121: The public Studio is a static browser-local bundle

- Status: Accepted
- Date: 2026-11-27
- Related: ADR-0118 (browser-local Studio), ADR-0119 (GB2312 subsets),
  ADR-0120 (content-hashed assets), ADR-0117 (reverse-proxy origins)
- Touches: `crates/soma-studio-ui/src/local.rs`, `state.rs`, `app.rs`,
  `shell.rs`, `verify.rs`, `scripts/static.sh`, `docs/studio.md`,
  `SECURITY.md`, deployment `somadoc-art-ssl.conf`

## Context

The engine already ran entirely in wasm on the public host: parsing,
template resolution, K-P layout, pagination, SVG proof, PDF and TikZ. Axum was
kept behind the same Apache vhost and still exposed `/api` publicly. The API
has no authentication: `/api/pdf` can burn CPU and `/api/file` writes inside
the project root. `Origin` validation is not authentication; a request without
an `Origin` header passes.

Two browser-side leaks made a static deployment unsafe even after removing the
proxy:

1. `is_local()` was a host allowlist (`somadoc.art`). Any other host — a CDN
   preview, a custom domain, an IP — defaulted to **server** mode and called
   `/api`.
2. Local verification wrote the wasm summary into the `verify` slot and the
   chrome compared it with itself, showing `wasm = native`. No native process
   participated.

Fonts were deliberately not part of the wasm: the GB2312 subsets are 4.4 MB
and the proof/PDF share the same bytes.

## Decision

1. **The public Studio is static.** Apache serves `dist/` as its
   `DocumentRoot`; there is no `ProxyPass`. `somadoc.service` is a
   development-only host and is not part of the public deployment. `/api` is a
   404 by construction.
2. **Local is the default on every non-loopback host.** `?server=1` is the
   explicit development escape hatch; `?local=1` forces local mode on loopback
   for the static gate. Loopback keeps the server workflow (file tree, native
   verify).
3. **Local mode opens files in the browser.** `.md`, `.html`, `.tex` and
   `.docx` go through the file picker or window drop; bytes stay in wasm and
   never touch a server. Text is decoded as UTF-8; DOCX keeps its archive in
   the tab and is parsed by `soma-docx` on each rebuild.
4. **Local verify is a build summary, not a runtime native check.** The pane
   shows the wasm canonical hash, parameter hash, pages and per-page lines.
   Native/wasm equality is a CI property (`scripts/wasm-hash.sh`); the local
   chrome says `wasm 构建`, never `wasm = native`.
5. **Fonts stay external and versioned.** `/fonts/v2/` is served as static
   files, shared by `@font-face` and PDF. Embedding fonts would make first
   paint download all faces and turn a font revision into an engine revision.
6. **Static hosting owns the headers.** Apache sets the CSP, HSTS, nosniff,
   referrer policy, long-lived caching for hashed assets and fonts, and
   `no-cache` for `index.html`. The hashed JS/CSS/wasm names come from
   ADR-0120.
7. **A gate locks the shape.** `scripts/static.sh` serves `dist/` without
   Axum, runs S1/S3–S10/S12/S13 with `?local=1`, asserts that the session makes no
   `/api` request, and asserts `/api/health` is 404.

## Consequences

- The public surface has no API, no filesystem writes and no server CPU to
  abuse. Layout and PDF cost is paid by the visitor's browser.
- The public deployment has no server logs for usage; analytics, if needed,
  are a separate static-friendly decision.
- A new hostname or CDN needs no Rust change: local is the default.
- Server mode remains available on loopback for development, with its existing
  warning that `--host` exposes unauthenticated read/write routes.
- GB2312 coverage (6 763 characters) remains the browser boundary (D119). A
  character outside the subset renders as a box; the CLI still has full faces.
- DOCX parsing is now reachable from the public page, but only from a file the
  visitor chooses; there is no upload endpoint.
- The public bundle is still not a single file: `index.html`, hashed assets and
  `/fonts/v2/` ship together. A future `embed-fonts` build is optional and
  does not change this ADR.

## Alternatives considered

- **Keep Axum and deny `/api` in Apache.** Rejected as the primary shape: it
  leaves an application server on the public host for no browser-local
  functionality, and a future vhost edit could re-expose it.
- **Compile out the API with a `public` feature.** Deferred: a 404 is already
  a hard boundary, and cfg branches across `state/app/verify/shell` add more
  maintenance than they remove today. Revisit if the `/api` strings in the
  wasm become a compliance requirement.
- **Embed the fonts in the wasm.** Rejected: first paint grows from ~4 MB wasm
  plus on-demand fonts to ~8.5 MB before the engine starts; font revisions
  invalidate the engine payload; `@font-face` still needs Blob URLs.
- **Keep a server-side file tree for public users.** Rejected: it requires
  authentication, storage and cleanup. A downloaded/uploaded single file is
  the product boundary, matching ADR-0119.
