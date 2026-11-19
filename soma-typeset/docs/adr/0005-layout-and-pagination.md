# ADR-0005: Layout Box Tree; vertical K-P + greedy fallback

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §8, §10

## Context

The frozen K-P kernel handles one paragraph at a time. Documents need pages,
page masters, headers/footers, footnotes, figures, tables, and block spacing.
These are vertical concerns and cannot be added to `Paragraph` without mixing
line breaking with pagination.

## Decision

Introduce a Layout Box Tree between computed styles and algorithms:

```text
VBox / HBox / ParagraphBox / TextRun / ImageBox / MathBox
Glue / Penalty / FootnoteArea / PageBreak
```

- `ParagraphBox` wraps the existing `Atom + Boundary` sequence; the K-P kernel
  is reused unchanged.
- A paragraph's `DocIR` content is compiled by `soma-rules` into the inline
  sequence, with `StyleId` from computed styles.
- Pagination is **vertical K-P** over VBox breakpoints, with a deterministic
  greedy fallback when no feasible page sequence exists.
- Widows/orphans, keep-with-next, page-break-before/after are penalties or hard
  constraints in the vertical model.
- Footnotes are laid out as a block per page; the page is re-laid out when the
  footnote area height changes. Iteration is capped (8) and must converge
  deterministically or emit a diagnostic with the last stable layout.
- Images/math are boxes with host-provided metrics; floats are out of scope.
- All geometry is integer; no floats in layout.

## Consequences

- Line breaking and pagination share the idea of cost/penalty but are separate
  DPs with separate version numbers.
- Page results are page-structured; the old line `LayoutResult` remains for
  regression and the inline kernel.
- Footnote iteration and page-master switching need dedicated golden tests.
- Greedy fallback guarantees a complete result; `fallback_stage` must say when
  it was used.

## Alternatives rejected

- **Greedy pagination only**: simpler, but breaks the project’s global-optimization
  identity and makes widow/orphan handling ad hoc.
- **Put pages into `soma-core`**: bloats the `no_std` kernel and couples line
  breaking to page templates.
- **Float layout in MVP**: too large; reserve boxes and add later.

## Alignment with current code

- `crates/soma-core/src/breaker.rs` and `params.rs`: reused for `ParagraphBox`.
- `crates/soma-core/src/result.rs`: line result; page result added separately.
- `crates/soma-rules/src/lib.rs`: inline sequence compiler; unchanged.
- `scripts/bench.sh` / `soma-eval`: extend to page benchmarks later.
