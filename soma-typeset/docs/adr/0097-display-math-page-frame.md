# ADR-0097: Display math percentages resolve against the page

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0094 (HTML geometry is percentage-based), ADR-0076 (math
  geometry), ADR-0093 (Web Studio)
- Touches: `soma-render-html`, its math tests; no canonical layout, PDF or SVG
  change

## Context

ADR-0094 replaced every absolute HTML coordinate with a percentage of the
positioning context: block geometry is a percentage of the page, and inline
atom geometry is a percentage of its block. `paint_math` was given the same
block frame for both display and inline formulas.

Display math is emitted directly into the page body, not inside a positioned
`.block` wrapper. Its `left`/`top` percentages therefore resolved against
`.page`, while the numbers were computed against the formula block. On the math
page the block is narrower than the page, so every glyph and rule was displaced
by the page/block ratio: formula parts flew across the page, and TikZ paths
turned into large rectangles. The PDF and SVG were correct because they use
absolute coordinates, and the bug only appeared in the Studio proof.

## Decision

- **Display math uses the page frame.** `render_block` passes the page frame to
  `render_math` for `BoxKind::Math`, because the spans land directly in the
  page body.
- **Inline math keeps the block frame.** Inline formulas are emitted inside the
  positioned `.block`, so their percentages must resolve against that block.
- The doc comment on `render_math` states which frame each caller passes.

## Consequences

- The Studio proof places display formulas, their rules and TikZ paths at the
  same page coordinates as the PDF; the scattered-glyph symptom is gone.
- HTML output for display math changes (`left`/`top` percentages are now page
  relative). No canonical layout, PDF, SVG, hash, template or parameter change.
- The HTML math tests now compute the expected percentages from the page frame
  plus the block offset, and would fail if display math went back to the block
  frame.
- The formula's internal glyph placement still uses the 7/8 em ascent proxy, so
  a mixed-font formula can be a fraction of an em off inside its own box; that
  is D101/D97 territory, not a page-scale drift.

## Alternatives

- **Wrap display math in a positioned `.block` div.** Would keep block-relative
  percentages, but adds a wrapper, duplicates the block frame, and changes the
  DOM shape the page overlay and S gate rely on.
- **Keep block-relative numbers and divide by the page/block ratio.** More
  arithmetic and easy to forget in the rule and TikZ paths.
