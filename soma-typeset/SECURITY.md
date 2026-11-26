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

- **Local by default.** `soma-studio` binds `127.0.0.1` unless `--host` is
  changed. The default origin allowlist is loopback only.
- **Reverse proxy.** For a public deployment, terminate TLS at a proxy and pass
  the public names with `--allow-origin`. Do not expose port 7070 directly to
  the internet; the Studio has no user authentication.
- **API boundary.** `/api` validates `Origin` and `Sec-Fetch-Site`; file paths
  are resolved inside the project root; writes are limited to the configured
  tree. Treat the Studio as a single-user authoring tool.
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
- The local Studio is a single-file editor; arbitrary file access is only in
  server mode (`?server=1`).
- The engine does not execute user-provided code, fetch remote URLs, or open
  fonts from an untrusted path at runtime. DOCX/HTML/LaTeX parsing is
  dependency-free and bounded by the input size.
