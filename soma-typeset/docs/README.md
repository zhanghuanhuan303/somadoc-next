# Documentation

This directory is the project's research and engineering record. It mixes
normative contracts with historical reports; the distinction matters.

## Start here

| Document | Role |
|---|---|
| [`soma-atlas.html`](soma-atlas.html) | Single-file current map: capability, architecture, gates, numbers, pitfalls, boundaries |
| [`architecture-v2.md`](architecture-v2.md) | Frozen layer contract and dependency rules |
| [`adr/README.md`](adr/README.md) | All accepted decisions (ADR-0001–0120) |
| [`debt.md`](debt.md) | Explicit not-done register (D01–D119, N1–N6) |
| [`studio.md`](studio.md) | The browser/Studio boundary, flags and security |
| [`engine.md`](engine.md) | `soma.engine/1`: presets, windows, fallback |
| [`docir.md`](docir.md) | `soma.doc/3` contract |
| [`template.md`](template.md) | Pure-YAML template DSL |
| [`math.md`](math.md) | Display and inline math subset |
| [`tikz.md`](tikz.md) | TikZ subset and backends |
| [`cli.md`](cli.md) | The `soma` command line |
| [`testing.md`](testing.md) | Test layout and gate map |
| [`capabilities.md`](capabilities.md) | Frontend capability matrix |
| [`corpus-calibration.md`](corpus-calibration.md) | Corpus and calibration numbers |

## Historical / non-normative

The `a*-report.md`, `k*-report.md`, `e1-q-report.md`, `proposal-*.md`,
`blind-*.md`, `first-release.md`, `roadmap-v2.md` and `archive/` files record how
the project got here. They are useful context, but if they disagree with the
atlas, an ADR, or the code, the latter wins.

## Open-source notes

- `docs/deploy/` is an example deployment unit, not a hosted configuration.
- No credentials, private host addresses, or generated PDFs belong here.
- The browser font subsets are generated into `dist/` and are not committed;
  `scripts/local-assets.sh` documents the expected faces.
- Reports may mention earlier schema versions or test counts. The atlas is the
  dated snapshot; `cargo test` and `scripts/gates.sh` are the live numbers.
