# ADR-0100: `--host` opens Studio beyond loopback and the Origin allowlist follows it

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0093 (Web Studio), ADR-0095 (Studio builds the active tab)
- Touches: `soma-studio` (options, binding, `check_origin`), `docs/studio.md`,
  the Studio gate

## Context

The Studio server was hard-wired to `127.0.0.1`. The bind address said so, and
`check_origin` only accepted `http://127.0.0.1:<port>` and
`http://localhost:<port>`. Running the binary on another machine and opening
`http://192.0.2.10:7070/` loaded the UI, but every `/api` call returned 403
`cross-origin request refused`, so the page reported a failed build.

Changing only the bind address would be worse than useless: the browser would
still be rejected, and the temptation would be to delete the Origin check
entirely. The Origin check is what stops a page on another site from driving
`/api/file`, `/api/rebuild` or `/api/pdf` against a Studio the developer is
already running.

## Decision

- **`--host <addr>` is an explicit opt-in.** The default stays `127.0.0.1`;
  omitting the flag reproduces the previous local-only behaviour byte for byte.
  The address is bound as `host:port` (IPv6 literals are bracketed).
- **The Origin allowlist is derived from the configured host.**
  - Loopback names are always allowed for the configured port:
    `http://127.0.0.1:<port>`, `http://localhost:<port>`,
    `http://[::1]:<port>`.
  - With a concrete non-loopback host (e.g. `--host 192.0.2.10`) the origin
    `http://192.0.2.10:<port>` is added. No other origin is accepted.
  - With a wildcard host (`0.0.0.0`, `::`) the origin must equal
    `http://<Host header>`: the request is accepted only if the browser
    addressed the same authority it claims as its origin. A mismatched
    `Origin` (for example `http://evil.example:7070`) is still refused.
- **`Sec-Fetch-Site: cross-site` is still refused** for `/api`, regardless of
  the configured host. The CSP keeps `connect-src 'self'` and
  `frame-ancestors 'none'`.
- Loopback mode deliberately does **not** consult the `Host` header, so DNS
  rebinding against the default local server cannot widen the allowlist. A
  wildcard bind is a documented exposure (see below), and a concrete LAN IP is
  the recommended way to serve other machines.
- The startup banner prints the URL a browser can use: the configured host, or
  `127.0.0.1` for a wildcard bind, plus a second line when every interface is
  bound.

## Consequences

- `soma-studio --host 192.0.2.10 --port 7070` serves the UI and its `/api` calls
  from `http://192.0.2.10:7070/`, and `curl -H 'Origin: http://192.0.2.10:7070'`
  succeeds where it previously returned 403.
- `origin_allowed` becomes a pure function and gains unit tests: loopback
  stays local, a concrete host adds exactly itself, a wildcard bind follows the
  `Host` header, and an unrelated origin is rejected in every mode.
- **Wildcard risk, accepted and documented:** `--host 0.0.0.0` exposes the
  Studio — including `/api/file`, `/api/rebuild` and `/api/pdf` — to the
  reachable network. Anyone who can reach the port can read and write files
  under the project root. There is no authentication. Prefer a concrete LAN
  address, and do not bind a wildcard address on an untrusted network.
- No engine, template, DocIR or canonical layout change; hashes and the corpus
  baseline are untouched.

## Alternatives

- **Bind everything and allow every `Origin`.** Simplest, but it turns the
  local server into a cross-site file and PDF endpoint for any page the
  developer visits.
- **Require the operator to list allowed origins in a flag.** More precise, but
  it adds a second piece of configuration that must agree with `--host` and the
  port; deriving it from the bind address cannot drift.
- **Use the request `Host` header in loopback mode too.** Convenient behind a
  proxy, but it reintroduces DNS rebinding against the default configuration.
