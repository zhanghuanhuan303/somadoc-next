# Security policy

## Supported versions

The project is at `0.1.x`. Security fixes land on `main`; there are no
long-lived release branches yet.

## Reporting a vulnerability

Please do **not** open a public issue for a vulnerability. Use GitHub's private
security advisory form for this repository:

<https://github.com/zhanghuanhuan303/somadoc-next/security/advisories/new>

Include the affected version/commit, a reproduction, and the impact. If the
report involves a deployed instance, say whether the instance is the public
demo or a private host.

## Security model

- **The public Studio is static.** `somadoc.art` serves only `index.html`,
  content-hashed JS/CSS/wasm, `/fonts/v2/` and `welcome.md`. There is no Axum
  process and `/api` is a 404 (ADR-0121). Layout, PDF and TikZ run in the
  visitor's browser.
- **`is_local()` defaults to local on every non-loopback host.** A CDN preview,
  custom domain or IP address cannot silently fall back to `/api`; local
  requires no server. `?server=1` is the explicit development override.
- **Development host only.** `soma-studio` binds `127.0.0.1` unless `--host` is
  changed. It is not part of the public deployment. Do not expose port 7070
  directly to the internet; the Studio has no user authentication.
- **API boundary (development).** `/api` validates `Origin` and
  `Sec-Fetch-Site`; file paths are resolved inside the project root; writes are
  limited to the configured tree. `Origin` validation is not authentication.
  Treat the server-mode Studio as a single-user authoring tool.
- **Runtime assets.** The browser build fetches only same-origin static assets
  (wasm, CSS, GB2312 font subsets, sample figures). The CSP is
  `default-src 'self'`. A CDN deployment must add its asset origin to the CSP
  deliberately.
- **Secrets.** Credentials never belong in the repository, the wasm, or the
  frontend. `.env` files are ignored. Deployment credentials live on the host,
  not in this tree.

## Known boundaries, not vulnerabilities

- Characters outside the GB2312 subset render as empty boxes in the browser
  (D119). This is a deliberate size trade-off; the CLI can use full faces.
- The static Studio opens only files the visitor explicitly picks or drops.
  Text is decoded as UTF-8 in wasm; DOCX is parsed in wasm. Nothing is
  uploaded. The development server (`?server=1`, loopback) still has its
  unauthenticated file API and must not be exposed publicly.
- The engine does not execute user-provided code, fetch remote URLs, or open
  fonts from an untrusted path at runtime. DOCX/HTML/LaTeX parsing is
  dependency-free and bounded by the input size.
