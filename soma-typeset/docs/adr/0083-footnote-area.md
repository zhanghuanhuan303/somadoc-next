# ADR-0083: The footnote area gets a rule, a number column, and a correct superscript

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0017 (table grids and headers), ADR-0031 (alignment), ADR-0081
  (reference template), `docs/template.md`
- Touches: `soma-layout` (`Page`, `compile_notes`, `pagination`, `Numbering`),
  `soma-render-pdf`, `soma-render-html`, `soma-render-svg`,
  `crates/soma-layout/tests/a6_blocks.rs`, `crates/soma-render-svg/tests/gate_r.rs`

## Context

The reference PDF exposed three defects in the footnote area:

1. There was no separator between the body and the notes.
2. The note itself had no number; only the body carried a mark, and in the PDF
   the number was drawn high above the line.
3. The body mark used `y += full / 3`, where `full` is the *milli-point* size
   passed to the PDF font operator. The baseline `y` is in layout units, so the
   raise was about 9pt instead of about 2pt.

The note text also started at the left text edge, leaving no column for the
number.

## Decision

### Separator

The layout reserves a separator for every page with at least one footnote:

- thickness `1/32 em` (`FOOTNOTE_RULE_THICKNESS`),
- gap below it `1/4 em` (`FOOTNOTE_RULE_GAP`),
- width one third of the content width (`footnote_rule_width`).

`pagination::note_height_of` includes the rule and gap in the page reservation,
and `assemble_footnotes` starts the first text block below them. The renderers
draw the rule at the top of the footnote area; it is not a `PlacedBlock`, so
the existing "one block per note paragraph" shape and tests stay intact.

### Number column and note numbers

`compile_notes` reserves `footnote_number_column(font_size) = 1.5 em` at the
left, narrows the note measure by that amount, and shifts every placed note
block right by the column. The number is drawn by the renderer at the content
left edge, so it never takes part in line breaking and multi-digit numbers
still fit.

`Page` gains two derived fields:

- `footnote_notes: Vec<usize>` — the definition index of each placed note
  block, aligned with `footnotes`;
- `footnote_numbers: Vec<Option<u32>>` — the printed number of each block, with
  `Some` only on the first block of a definition.

`Numbering::assign` now records the numbers it gives to the body marks on a
page and assigns the same sequence to the distinct notes in first-reference
order. Both fields are derived and are not written to the canonical page
result, so `soma.pages` keeps its shape and hash contract for the same layout.

### Superscript geometry

A body footnote reference is raised by `font_size / 5` in **layout units**, and
a note number by the same amount. The font size passed to the PDF operator
stays in milli-points; only the baseline shift is layout units.

## Consequences

- Every page with a note has a short rule above it and a numbered note; the
  body mark sits just above the cap height instead of floating.
- Footnote text is laid out 1.5 em narrower. A long note can gain a line and a
  page can move; the corpus `footnotes` document stays at 2 pages and `ALL`
  stays at 48 pages.
- HTML, SVG and PDF consume the same `footnote_numbers`; the SVG golden hash
  changes because the rule and number are new painted content.
- `Page` equality includes the two derived vectors; code that constructs a
  `Page` literal must fill them. They are deliberately not canonical.

## Alternatives

- **Prepend the number as a text atom.** It would need the number before line
  breaking, but page/section numbering is only known after pagination; and a
  second pass would change the note's line count. The left column avoids both.
- **Draw the number as a page decoration without layout cooperation.** The text
  would overlap the number column and the page reservation would be short;
  rejected.
- **Use the body em for the separator instead of a fixed fraction.** Then a
  change in body size would not scale the rule; the em fraction is the same
  convention as the table rules.

## Alignment

- `crates/soma-layout/src/lib.rs` — constants, `compile_notes`, `Page`,
  `Numbering::assign`.
- `crates/soma-layout/src/pagination.rs` — reservation and assembly.
- `crates/soma-layout/tests/a6_blocks.rs` — rule/column/number test.
- `crates/soma-render-pdf/src/lib.rs` — separator and number painters.
- `crates/soma-render-html/src/lib.rs`, `crates/soma-render-svg/src/lib.rs` —
  the corresponding preview output.
