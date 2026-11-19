# ADR-0007: Freeze the inline K-P kernel

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §15.2, K1–K8 reports

## Context

K1–K8 delivered a verified inline kernel:

- `soma-core`: units, metrics seam, parameters, K-P/greedy, trim, forced
  segments, canonical line result;
- `soma-rules`: segmentation, punctuation classes, gaps, trim caps, hard-break
  helper;
- verification: brute-force optimum, canonical bytes, native/wasm hash,
  performance budget.

The document/template/page work could tempt a rewrite “to make it fit”. That
would discard the strongest verified asset and reopen settled questions
(tie-break, badness normalization, fallback, canonical bytes).

## Decision

Freeze the following as the **inline kernel**:

- `crates/soma-core/src/breaker.rs`
- `crates/soma-core/src/params.rs`
- `crates/soma-core/src/result.rs`
- `crates/soma-core/src/ir.rs`
- `crates/soma-core/src/units.rs`
- `crates/soma-core/src/metrics.rs`
- `crates/soma-rules/src/lib.rs`

Rules:

- New document/template/page features go into `soma-doc`, `soma-template`,
  `soma-layout`, `soma-*` frontends/renderers — never into the kernel.
- The kernel may receive bug fixes and performance-only changes that preserve
  canonical bytes and all invariants.
- K7 (fitness/rhythm) remains optional and out of the frozen baseline.
- Any change to the kernel’s model/schema requires a new ADR and a version bump.

## Consequences

- The inline result (`soma.layout/6`) remains a stable regression oracle.
- `soma-layout` wraps `break_paragraph` rather than reimplementing it.
- Page-level optimization is a separate DP, not a kernel change.
- Kernel tests and benchmarks stay meaningful across architecture work.

## Alternatives rejected

- Rewriting K-P for pages: loses verified behavior and duplicates the algorithm.
- Adding page/template types to `soma-core`: breaks `no_std` focus and couples
  layers.
- Allowing silent kernel changes during frontend work: makes regressions
  untraceable.
