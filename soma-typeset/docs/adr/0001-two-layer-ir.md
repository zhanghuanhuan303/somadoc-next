# ADR-0001: Two-layer IR (DocIR + Layout Box Tree)

- Status: Accepted
- Date: 2026-11-20
- Supersedes: the single-pipeline assumption in `docs/archive/proposal-phase1-v1.md`
- Related: `docs/architecture-v2.md` §4–§8

## Context

The implemented K1–K8 pipeline is:

```text
text -> soma-rules -> Paragraph(Atom + Boundary) -> K-P -> LayoutResult
```

`Paragraph` is a paragraph-level inline layout input. It has no document
structure, no page geometry, no template styles, and no vertical layout.

Adding a document model, templates, pages, and multiple syntax front ends to
that single layer would couple three different concerns:

- syntax semantics (what the content means);
- presentation (how the template styles it);
- physical layout (widths, baselines, pages).

## Decision

Use two IRs, plus `ParaIR` as an inline sublayer:

1. **DocIR** (`soma-doc`): block/inline semantics, source spans, roles/classes,
   footnotes, figures, tables, math, page/section breaks. No widths, fonts, or
   pages.
2. **Layout Box Tree** (`soma-layout`): VBox/HBox/ParagraphBox/TextRun/Glue/
   Penalty, computed styles, page masters, footnotes.
3. **`ParaIR`** (`soma-core`): the existing `Atom + Boundary` sequence. It lives
   inside a `ParagraphBox`; the frozen K-P kernel consumes it unchanged.

Template resolution sits between DocIR and the Box Tree; it never sees syntax
ASTs. K-P and pagination only see the Box Tree.

## Consequences

- Syntax front ends can be added without touching layout.
- Templates can restyle content without changing DocIR semantics.
- The existing K-P kernel and `LayoutResult` remain valid and testable.
- Two canonical schemas and version numbers are required.
- More types and conversion code, but each layer is independently testable.

## Alternatives rejected

- **One IR for everything**: templates, syntax, and layout would share mutable
  concerns; a template change could alter break units.
- **Template emits boxes directly**: bypasses semantic validation and makes
  content/template separation impossible.
- **Put DocIR into `soma-core`**: bloats the `no_std` kernel and mixes document
  concerns with line breaking.

## Alignment with current code

- `crates/soma-core/src/ir.rs`: `Paragraph`/`Atom`/`Boundary` stay as the inline
  sublayer.
- `crates/soma-core/src/breaker.rs`: unchanged.
- `crates/soma-core/src/result.rs`: line result stays; page result is added in
  `soma-layout`.
- `crates/soma-core/src/semantic.rs`: legacy seed, not the final DocIR; replaced
  by `soma-doc` (ADR-0008).
