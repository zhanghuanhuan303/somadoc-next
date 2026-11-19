# Archive

Files in this directory are **historical and not normative**. They are kept
only to explain how the project arrived at architecture v2. Do not use them as
design input.

| File | Status | Why archived |
|---|---|---|
| `proposal-phase1-v1.md` | Legacy | Early Phase 1 proposal; references a deleted prior project. Superseded by `docs/architecture-v2.md` and `docs/adr/`. |
| `proposal-phase1-k1-k8.md` | Legacy | K1–K8 execution plan. Superseded by architecture v2; the K1–K8 code remains the frozen inline kernel (ADR-0007). |

Current architecture: [`../architecture-v2.md`](../architecture-v2.md)
Current decisions: [`../adr/README.md`](../adr/README.md)

Clean-room policy: see ADR-0002. New code must not reference the deleted prior
project, even if an archived document mentions it for history.
