# Architecture Decision Records

Status: **frozen on 2026-11-20** with `docs/architecture-v2.md`.
ADR-0010 amends §15.1 (one extra crate) and fixes shapes that
architecture v2 left open; it does not change any earlier decision.

These ADRs record the decisions that shape the next phase. They are aligned
with the already-implemented K1–K8 code; they do not change it.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-two-layer-ir.md) | Two-layer IR: DocIR + Layout Box Tree | Accepted |
| [0002](0002-clean-room.md) | Clean-room; no prior Typst project | Accepted |
| [0003](0003-pure-yaml-template.md) | Pure YAML template; no CSS engine | Accepted |
| [0004](0004-syntax-frontends.md) | Frontend trait; semantic whitelist | Accepted |
| [0005](0005-layout-and-pagination.md) | Box tree; vertical K-P + greedy fallback | Accepted |
| [0006](0006-determinism-and-versioning.md) | Integer units, canonical JSON, hash tuple | Accepted |
| [0007](0007-freeze-inline-kernel.md) | Freeze the existing inline K-P kernel | Accepted |
| [0008](0008-docir-migration.md) | New `soma-doc`; retire legacy semantic module | Accepted |
| [0009](0009-semantic-presentation-boundary.md) | DocIR semantics; template owns presentation | Accepted |
| [0010](0010-borrow-first-and-parallel.md) | Borrow-first layer boundaries; optional parallelism outside the core | Accepted |

## Rules

- An accepted ADR is not edited to change its decision; add a new ADR that
  supersedes it.
- Every ADR states context, decision, consequences, alternatives, and the code
  or documents it aligns with.
- Architecture changes require a new ADR before code changes.
- Historical proposals live in `docs/archive/` and are not normative.
