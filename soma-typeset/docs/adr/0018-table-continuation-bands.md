# ADR-0018: Table continuation by row bands

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0013 (line-level vertical layout), ADR-0017 (table grids and
  repeated headers), `docs/debt.md` D51
- Supersedes: the "one atomic item per rowspan group" decision of ADR-0017
- Extends: the table branch of `soma-layout` and `pagination.rs`. No schema
  change.

## Context

ADR-0017 merged every run of rows connected by a `rowspan` into one atomic
`FlowItem`, so a table could only break between groups. That kept spanning
cells intact, but it also stopped a table from continuing when a band was
taller than the page: the page breaker had no legal break point and the whole
band went to the overflow path. D51 still listed cell-content continuation as
open.

The box model is flat: a cell is a set of `PlacedBlock`s (paragraph slices)
positioned inside the table, not a nested container. There is no cell-local
pagination state, so "split a cell" can only mean "shift a prefix of its
blocks to the next page".

## Decision

### A table is one flow item with row-band parts

Row heights are resolved first, exactly as in ADR-0017. Then each boundary
between row `r` and row `r + 1` is classified:

- **unsafe** if any cell with `rowspan > 1` covers both rows;
- **unsafe** inside the repeated header (the header is never split);
- **safe** otherwise.

Maximal runs of rows between unsafe boundaries become **bands**. A band is one
`Part { height, row }` of the table's single `FlowItem`, and `row` is the first
row it covers. A table whose bands are all safe boundaries produces one part
per row; a table with no safe boundary produces one part and stays atomic.

If a spanning cell crosses the header boundary itself, the header could not be
repeated without duplicating body content, so that table is emitted as one
atomic part.

### A break point is always a row boundary

Because parts are bands, every page break inside a table falls between two
complete rows. Every cell block then lies entirely above or entirely below the
cut, so a continuation page can be assembled by copying the blocks whose
`y` crosses the cut and subtracting `cut` from their `y`. No line, and
therefore no `atom`, is ever clipped. The slice also has an upper bound
(`cut_end`, the height of the bands placed on this page), so rows are neither
duplicated on the next page nor painted early.

### Header repetition follows the part's first row

`TableContext::row` now carries the first row of the **part** rather than the
first row of the flow item. `header_reserve` repeats the header whenever a
page starts at a row after the header, including a page that starts inside the
first band. Breaking directly after the header band leaves the header alone on
the page, so that break carries the vertical club penalty; table parts do not
carry widow penalties (a single trailing row is not a widow).

### The header height is outside the natural fill

`assemble_page_blocks` subtracts the repeated header height from the page's
free space before distributing stretchable glue. Previously the reserve was
counted by the page DP but not by the stretch calculation, which could push
content past the content box on a continuation page.

## Consequences

- D51 narrows to nested `VBox`/`HBox` and row rules; cell content now continues
  across pages.
- A table taller than a page continues instead of overflowing, as long as a
  single band fits a page. A single band taller than the page still reports
  overflow through the existing atomic path.
- `soma.pages/3` is unchanged. `Page.blocks` stays flat: continuation rows are
  ordinary `TableCell` blocks with shifted `y`, and the repeated header is
  still a plain copy of the header blocks.
- `FlowItem` and `VItem` keep their shapes; `Part` gains `row`, and the table
  `FlowItem` now spans the whole table instead of one group.

## Alternatives rejected

- **Clip the lines of a spanning cell at the cut.** Loses glyphs mid-line and
  breaks the "one atom = one glyph" invariant of the renderers.
- **Re-lay out the remaining cell content on the continuation page.** Needs a
  page-aware cell height and a second layout pass; that is a different model
  version, not a pagination detail.
- **Keep ADR-0017 groups and only split between them.** Correct but leaves
  tall bands (and therefore many real tables) overflowing.
