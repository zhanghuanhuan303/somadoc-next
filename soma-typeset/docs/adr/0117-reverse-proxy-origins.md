# ADR-0117: Named public origins for a reverse-proxied Studio

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0109 (SVG proof), `soma-studio` origin check
- Touches: `soma-studio` (`Options`, `parse_args`, `origin_allowed`), tests,
  deployment `run.sh`

## Context

The Studio accepts writes and PDF exports on `/api/*`, and `check_origin`
rejects an `Origin` that it cannot account for. The check was written for a
direct bind: loopback authorities for the configured port, a concrete LAN host,
or — when bound to a wildcard — the authority in the request's `Host` header.
All comparisons used the `http` scheme.

Deploying behind Apache on `https://somadoc.art` breaks that assumption. Apache
terminates TLS on 443 and forwards the request to `127.0.0.1:7070` with
`Host: somadoc.art`; Chrome sends `Origin: https://somadoc.art`. The origin did
not match any allowed form, so every `POST /api/pdf`, `/api/rebuild` and
`POST /api/file` returned `403 cross-origin request refused`, while the page
itself loaded.

## Decision

1. `soma-studio` gains a repeatable `--allow-origin <host[:port]>` flag. Each
   value is a bare authority (no scheme, no path); anything else is a parse
   error. `/api` accepts an `Origin` equal to `http://<value>` or
   `https://<value>`.
2. The deployment passes the public names explicitly:
   `--allow-origin somadoc.art --allow-origin www.somadoc.art`. The Studio still
   binds `127.0.0.1`; only Apache can reach it.
3. The default remains loopback-only. A wildcard bind still follows the request
   `Host`, and a concrete bind still adds exactly that host.
4. `X-Forwarded-Proto` is not consulted. The proxy must set `ProxyPreserveHost
   On`; the allowlist is scheme-agnostic because both HTTP and HTTPS are
   considered first-party for a name the operator owns.

## Consequences

- Saving, rebuilding and exporting PDF work through the TLS proxy without
  widening the default threat model for desktop users.
- The flag is a static host allowlist, not a trust-any-proxy switch. A
  misconfigured proxy that forwards a foreign `Host` still fails the check
  unless that name was named on the command line.
- No change to `DocIR`, layout, canonical JSON, `modelVersion` or `rulesVersion`;
  the native/wasm hash is unaffected.

## Alternatives considered

- **Accept `https` for loopback origins unconditionally.** Rejected: the
  browser's origin is the public name, not `127.0.0.1`, so this does not fix the
  proxy case.
- **Accept the request `Host` for any bind.** Rejected: while convenient, it
  turns the loopback-only default into a DNS-rebinding target for a hostile
  page that resolves its own name to `127.0.0.1`.
- **Bind `0.0.0.0` and rely on the cloud security group.** Rejected: the host
  firewall is inactive on the target machine; exposing the Studio port directly
  bypasses the proxy and its TLS.
- **Parse `X-Forwarded-Proto`.** Rejected for now: it trusts a header the proxy
  must set anyway, and the allowlist does not need a scheme distinction.
