# ADR-0085: An underline run covers its justification glue

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0044 (PDF decorations), ADR-0081 (reference template),
  ADR-0082 (line-start trim)
- Touches: `soma-render-pdf` (`decoration_advance`, decoration painting),
  `crates/soma-render-pdf/src/lib.rs` unit test

## Context

`下划线` in the reference sample was drawn as three separate rules with visible
gaps. The PDF renderer painted each atom's underline as a rectangle of exactly
`atom.advance`. A justified line distributes its adjustment as glue **between**
atoms, so two adjacent underlined atoms are farther apart than
`first.advance`, and the gap between their rules is the adjustment. The same
problem applies to a strikethrough.

The outline and the text index are correct; this is a painting bug, not a
layout bug.

## Decision

`decoration_advance(boxes, index, decorated)` returns the atom's own advance,
extended to the next atom's `x` when that neighbour carries the same
decoration. The rule therefore covers the glue between two underlined atoms and
touches the next rule. A non-decorated neighbour ends the run; the last atom of
a run keeps its own advance. The PDF renderer uses this for underlines and
strikethroughs.

HTML and SVG continue to use CSS/SVG `text-decoration`; a preview may still
show per-atom gaps because those formats position atoms absolutely. Closing
that gap needs a separate run-level decoration box and is left out of this
change.

## Consequences

- An underlined CJK run has one continuous rule across justification glue; the
  reference sample's `下划线` is unbroken.
- A run that starts or ends at a non-decorated atom keeps a clean edge: the
  rule never extends into a neighbour that is not underlined.
- The decoration geometry is testable without parsing a PDF stream; the unit
  test pins the extended and terminated cases.
- The canonical page result and the text index are unchanged.

## Alternatives

- **Merge all consecutive underlined atoms into one rectangle in the painter.**
  Equivalent output, but it needs a run scanner and a separate rectangle per
  run; the per-atom extension is the same geometry with less state.
- **Extend every underline to the next atom regardless of decoration.** It
  would bleed into non-underlined text; rejected.
- **Change the layout to insert tracking.** The gap is justification glue and
  is wanted; the decoration must follow it, not the other way round.

## Alignment

- `crates/soma-render-pdf/src/lib.rs` — `decoration_advance` and the two
  decoration calls.
- `crates/soma-render-pdf/src/lib.rs` — `an_underline_run_bridges_justification_glue`.
