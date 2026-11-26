# ADR-0114: The default Plex template, finished for proposals

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0088 (Plex as the built-in CLI default),
  ADR-0106 (book layout), ADR-0110 (colophon block), ADR-0113 (diagram fit)
- Touches: `examples/templates/plex.yaml` (template version 2),
  `crates/soma-template/tests/a2_reference_hashes.rs`, `docs/template.md`,
  the Studio deployment

## Context

A review of the TACTUS proposal (rendered with the Plex template) found the
body type and headings sound, but three presentation details below the level of
the line breaker:

- capability tables were dense: rows sat on the body line height with no
  vertical padding, and a table could touch the paragraph that introduced it;
- the contents page began flush at the top of the type area, and the cover's
  colophon inherited the body's left alignment, so its rule and lines collided
  with the author/date block;
- the review asked for a more deliberate contents/cover transition.

`examples/templates/plex.yaml` is included by both `soma-cli` and `soma-studio`
as their built-in default (ADR-0088), so finishing it also finishes the default
output; a separate proposal template would have forked the design for no
reason.

## Decision

Template version 2 keeps the reference typography (10 pt / 14.4 pt body, 20 pt
paragraph skip, article heading sizes, booktabs rules) and changes only the
three weak spots:

1. **Tables breathe.** A cell paragraph uses a 1.5 line height and a symmetric
   `0.12em` space before/after; a table gets `0.8em` before it instead of
   `0.3em`. Widths, rules and column allocation are untouched.
2. **The contents page has a top.** `tocTitle` is 18 pt, sits `16mm` below the
   margin and has `1.8em` after it; entries use a 1.9 line height.
3. **The cover closes deliberately.** `coverTitle` is 26 pt with `34mm` before,
   the subtitle and date are muted (`#3c3c3c`, `#555555`), and the colophon is
   a centred 8 pt block with `0.04em` tracking under a 40% `#b3b3b3` hairline
   `12mm` below the date. Before this decision `coverColophon` and
   `coverColophonRule` had no styles in the Plex template, so the block
   inherited the body's left alignment.

## Consequences

- Documents that use the built-in template get the new table, contents and
  cover treatment; `soma render` and the Studio share it because both include
  the same file.
- The reference-template canary (`a2_reference_hashes`) moves to the v2 hash
  and byte length. `examples/book/song.yaml` is unchanged, so the book keeps its
  own design.
- The Studio deployment must copy the new `plex.yaml`; the metrics table does
  not change with style declarations.
- A future template change still needs a new ADR, because the default output
  moves with it.

## Alternatives considered

- **Add a separate `proposal.yaml` and make it the default.** Rejected: it
  would duplicate ~360 lines of template for three localized changes, and the
  two files would drift.
- **Use `baseline-grid` to force a vertical rhythm.** Rejected for this round:
  the grid snaps line advances, not block spaces; enabling it would inflate
  heading line boxes without aligning the gaps between tables, headings and
  paragraphs.
- **Put the table padding in the table engine instead of the template.**
  Rejected: padding and line height are presentation, and the template already
  expresses cell paragraphs through `within: cell`.
