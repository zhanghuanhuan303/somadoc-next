# ADR-0008: DocIR migration; retire the legacy semantic module

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0001, ADR-0002, `docs/architecture-v2.md` §5, §15

## Context

`soma-core::semantic` and `soma-core::text` are early seed modules. They are
quarantined legacy code (ADR-0002): they are not part of architecture v2, and
their node set is too small for templates, pages, footnotes, math, or figures.
They are currently only used by their own tests; no other crate depends on them.

## Decision

- Create a new crate `soma-doc` owning the clean-room DocIR.
- `soma-doc` v1 includes:
  - document metadata, sections, blocks, footnotes, source spans;
  - block nodes: paragraph, heading, list, quote, code, math, figure, table,
    thematic break, raw, generated blocks;
  - inline nodes: text, strong, emphasis, underline, strike, small caps, sup/sub,
    code, link, image, math, footnote ref, raw, ruby (reserved);
  - canonical JSON and schema version.
- **Do not copy or re-export the legacy types.** A1 removes
  `soma-core::semantic` and `soma-core::text` and their exports/tests.
- No new code may import `soma_core::semantic` or `soma_core::text`.
- `soma-md`/`soma-html`/`soma-latex` target `soma-doc`, not `soma-core`.
- `soma-core` remains the inline kernel (ADR-0007).

## Consequences

- A small breaking change at A1: legacy `BlockNode`/`InlineNode` and
  `extract_text`/`ir_to_markdown` disappear from the public API.
- The current test count drops by the two legacy text tests; new `soma-doc`
  tests replace them.
- No compatibility re-export is kept, to avoid a second, half-clean DocIR.
- `soma-doc` is designed from scratch and can include source spans and semantic
  roles from day one.

## Alternatives rejected

- **Extend `soma-core::semantic` in place**: keeps the legacy shape and violates
  the clean-room decision.
- **Re-export legacy types from `soma-doc`**: creates two sources of truth and
  preserves the old constraints.
- **Keep both indefinitely**: every frontend would need to choose a DocIR.
