# ADR-0050: a table is a real structure tree, five levels deep

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0017/0018 (table grids and continuation), ADR-0047 (tagged PDF),
  `docs/debt.md` D51
- Touches: `soma-layout` (`TextSource`, `TextIndex`), `soma-render-pdf`.
  **No schema change**: the grid position lives in `TextIndex`, which is not
  part of `soma.pages/9`.

## Context

ADR-0047 tagged every page, but a table cell became a `/P`. That was a
deliberate placeholder: `/TD` outside a `/Table` is an invalid tree, and the
renderer only saw a flat list of placed blocks with no idea which row or column
a cell belonged to. A screen reader could read a table as a run of paragraphs —
better than unlabelled glyphs, worse than a table.

The layout already knew the grid: `TableCellLayout` carries `row`, `column`,
`colspan` and `rowspan`, and it flattens each cell into placed blocks at a
computed `x`/`y`. The information existed; it just did not cross the seam.

## Decision

### 1. The grid position travels beside the box, not inside it

`TextSource` gains

```rust
fn block_cell(&self, _block: NodeId) -> Option<TableCell> { None }

pub struct TableCell {
    pub row: u16,
    pub column: u16,
    pub row_span: u16,
    pub column_span: u16,
}
```

and the layout records it while placing each cell's blocks (`mark_cell`), next
to the figure and heading metadata from ADR-0047. Keeping it out of
`PlacedBlock` is the same choice as there: `TextIndex` is not serialised, so the
page schema and its hash do not move, and the renderer still consumes only
`PagedLayout` + `TextSource`.

### 2. The structure tree is a tree, not a list

`PageTags` used to hold a flat `Vec<TagElement>`, one element per marked-content
sequence, all hanging off the Document. It now holds `TagNode`:

```rust
struct TagNode {
    tag: &'static str,
    alt: Option<String>,
    mcids: Vec<u32>,             // the sequences this element owns
    span: Option<(u16, u16)>,    // RowSpan / ColSpan
    children: Vec<TagNode>,
}
```

A container (`Table`, `TR`) owns children and no content; a leaf owns one or
more sequences. `open()` runs a small state machine: a block with a cell opens
`Table` if none is open, starts a `TR` when the row changes, and reuses the
current `TD` when the next block is another paragraph in the same cell. A block
without a cell closes the table.

Two consequences worth stating:

* **`/K` is the union.** An element writes `/K [child refs]` when it has
  children, `/K mcid` for a single sequence, `/K [mcids]` for several. A cell
  with two paragraphs is one `/TD` owning two MCIDs, which is what the spec
  means by a cell.
* **The parent tree is indexed by MCID, not by element.** `write_struct` returns
  the owner of every MCID it writes, the caller sorts by MCID, and
  `StructParents n` lists owners. Without this a `/TD` that owns two sequences
  would leave the second MCID pointing at a paragraph-shaped hole.

### 3. Spans are an attribute object

A cell that spans more than one row or column carries
`/A << /O /Table /RowSpan r /ColSpan c >>`. Spans of one are omitted: the
default is one, and writing `/RowSpan 1` on every cell is noise.

### 4. Continuation pages start a new table element

A table split across pages produces a second `/Table` on the next page. That is
not ideal — the logical table is one element — but the alternative is a
structure tree that spans pages the renderer never sees together, and a reader
handles two adjacent tables far better than one tree with dangling page
references.

## Consequences

- `examples/book` now has one `/Table`, four `/TR` and twelve `/TD`; the body
  paragraphs stay `/P`. `pdfinfo` still reports `Tagged: yes`.
- The unit test builds a two-row table with a spanning header cell and checks
  the tags, the span attribute, and that no cell is tagged as a paragraph.
- The renderer grew ~120 lines (the tree writer and the cell state machine) and
  lost the flat-list special case.
- D51 keeps its remaining items — nested VBox/HBox, per-table rule widths —
  but its "cells are paragraphs" caveat is gone.

## Alternatives

- **Put the grid in `PlacedBlock`.** Cleaner conceptually, and it would bump
  `soma.pages` to 10 and move every golden for a fact that only the structure
  tree consumes. The seam exists precisely so renderers can ask for more
  without the canonical result growing.
- **Group cells by their `y` and `x`.** Works until a rowspanning cell makes two
  cells share a top edge, or a continuation page starts mid-row. The layout
  knows the answer; guessing from geometry throws it away.
- **One `/TD` per placed block.** Simplest to write, and wrong: a cell with two
  paragraphs would be two cells.
