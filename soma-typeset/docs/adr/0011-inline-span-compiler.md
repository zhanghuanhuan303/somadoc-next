# ADR-0011: Inline span compiler in the rules layer

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0007 (frozen kernel), ADR-0009 (semantic/presentation boundary),
  `docs/architecture-v2.md` §15.2
- Amends: ADR-0007, by adding one entry point. No existing behaviour changes.

## Context

`soma-rules` compiles plain text into ParaIR: segmentation, CJK/Latin gaps,
punctuation classes, forbidden breaks, and trim caps all live there. The rules
layer is frozen (ADR-0007) and must stay the only place that decides those.

A3 needs ParaIR for **rich** inline content: `Strong`, `Code`, `Link`,
`InlineObject`, and per-run `StyleId`s from `StyledDoc`. The obvious shortcuts
are wrong:

- reimplementing segmentation inside `soma-layout` duplicates the rule layer and
  will drift from the text-only path;
- stitching several `paragraph_from_text` results loses cross-run glue, so
  `中文 **加粗**` would get the wrong CJK/Latin boundary at the run seam.

## Decision

Add one additive entry point to `soma-rules`:

```rust
pub enum SpanKind {
    /// Segmented by the normal rules; spaces collapse across spans.
    Text,
    /// One unbreakable atom (inline code).
    Code,
    /// One unbreakable atom (replaced object: image, math fallback).
    Object,
    /// A forced break between the previous and the next atom.
    HardBreak,
}

pub struct InlineSpan<'a> {
    pub text: &'a str,
    pub style: StyleId,
    pub kind: SpanKind,
}

pub fn paragraph_from_spans(
    spans: &[InlineSpan<'_>],
    style: ParagraphStyle,
) -> Result<Paragraph, RuleError>;
```

Implementation is a refactor, not a second algorithm: `paragraph_from_text`
becomes a thin driver over the same internal builder that
`paragraph_from_spans` uses. Boundary construction, pending-space propagation,
digit/latin run grouping, punctuation classes, trim caps, and `AtomKind`
assignment are shared by construction.

Rules:

- `pending_space` carries **across spans**, so a trailing space before a `Strong`
  run produces the same glue as the same space inside one span.
- `Code` and `Object` emit exactly one atom and never collapse internal spaces.
- `HardBreak` turns the next boundary into `Breakability::Forced`; a leading or
  trailing hard break is ignored, and consecutive hard breaks produce a single
  forced boundary because there is no empty line to place.
- An empty span list, or one with no visible atoms, returns `RuleError::EmptyText`.

## Consequences

- `soma-layout` never reimplements segmentation; it only maps DocIR inlines to
  spans and calls the rules layer.
- The existing text-only entry points keep byte-identical output: the K1–K8
  fixtures and the brute-force differential tests stay the guard.
- `soma-rules` gains one public function and one public data type. No schema,
  parameter, or canonical-format change; `soma.layout/6` is unaffected.

## Alternatives rejected

- **Segmentation in `soma-layout`.** Duplicates rule logic and the Gate A
  boundary ("rules belong to the rules layer").
- **Per-run compilation with manual stitching.** Wrong glue at run seams, and
  every future rule change would need two implementations.
- **Widening `Paragraph` with rich inline data.** Breaks the frozen ParaIR and
  every canonical golden.
