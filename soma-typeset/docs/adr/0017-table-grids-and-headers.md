# ADR-0017: Table grids (rowspan) and repeated headers

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0013 (line-level vertical layout), `docs/debt.md` D33/D51
- Extends: the table branch of `soma-layout`. No schema change.

## Context

DocIR has carried `colspan` and `rowspan` since A1, but layout only understood
`colspan` for width and ignored `rowspan` entirely. Validation checked each
row's colspan sum but not grid occupancy, so a document could declare
overlapping cells and reach layout. Tables taller than a page did not repeat
their header row, which makes multi-page tables unreadable.

## Decision

### Validation is a grid

`soma-doc::validate` builds an `rows × columns` occupancy map and places cells
like a layout engine would: skip columns already occupied from above, check the
colspan/rowspan rectangle is inside the table, reject overlaps, and require each
row to be full after both its own cells and the spans coming from above.

### A rowspan group is one atomic vertical item

Layout resolves cells first, then row heights:

1. single-row cells set their row's minimum height;
2. a spanning cell's deficit (content height minus the sum of its rows) is
   distributed across the rows it covers, last row taking the remainder;
3. rows connected by any span are merged into one **group**, and the group is a
   single `FlowItem`.

The group is atomic on purpose: a spanning cell whose content would be split by
a page break has no correct rendering without splitting cell content, which the
box model does not support. If a group is taller than the page, the existing
overflow path reports it. Groups also give a deterministic place to break:
between groups only.

### Headers repeat, but only at the top of a page

Leading `Row.header` rows form a header of height `H`. Every page that starts in
the middle of a table reserves `H` and prepends a copy of the header blocks.
This is part of the page DP, not a post-processing pass: the reservation is
counted in `used`, so a page never overflows because of a repeated header. The
header copy carries the original cell node ids, so `TextSource` resolves it
without new text entries.

A rowspan that crosses the header boundary cannot be repeated partially; such a
span stays in the first group and only pure header cells are copied.

## Consequences

- `FlowItem` gains `table: Option<TableContext>`; the page result gains no new
  fields because repeated headers are ordinary `PlacedBlock`s.
- Both the optimal and greedy paginators reserve the header height.
- Canonical `soma.pages/3` is unchanged. `Page.blocks` simply contains the
  header blocks before the content blocks.
- D33 closes (grid validation). D51 keeps only row rules and nested boxes.

## Alternatives rejected

- **One atomic item for the whole table.** Simple, but makes every table taller
  than a page an overflow.
- **Splitting cell content across pages.** Needs line-level cell boxes and a
  page-aware cell height; out of scope for this model version.
- **Post-processing header injection.** Would inflate `used_height` past the
  content box and could push content off the page.
