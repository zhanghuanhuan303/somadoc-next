# ADR-0065: relative column widths (and HTML `<colgroup>`)

- Status: Accepted
- Date: 2026-11-21
- Closes: the `colgroup` half of D69
- Related: ADR-0009 (DocIR carries no physical style), ADR-0058
  (content-sized columns), `docs/debt.md` D69
- Touches: `soma-doc` (`Column.share`), `soma-html` (`<colgroup>`/`<col>`),
  `soma-layout` (`column_widths`)

## Context

Columns were sized purely from content. That fixed the common "序号 / 说明"
failure, but an author who writes `<colgroup><col width="25%"><col></colgroup>`
has already said what the table should look like, and the engine ignored it.
DocIR deliberately has no physical lengths, so a `<col width="200px">` cannot
become 200 pixels in the IR.

## Decision

### 1. A share, not a length

```rust
pub struct Column {
    pub meta: NodeMeta,
    pub align: CellAlign,
    pub share: u16,   // per-mille of the measure; 0 = size from content
}
```

`share` is a proportion, so it survives a change of page size, font size, or
metrics table — the same reason `hang` and `tracking` are kernel fields rather
than template lengths. The field defaults to 0, is written to canonical JSON
only when non-zero, and therefore leaves every existing document's bytes
untouched (no schema bump; ADR-0007's rule).

### 2. HTML mapping

`<colgroup>` and `<col>` supply shares; `span` repeats a hint. `width="25%"`
becomes 250, `width="120"`/`"120px"` becomes 120. Anything else (em, cm, `*`)
is ignored, and a table without a `<colgroup>` has all-zero shares.

### 3. Layout

- All shares zero: the previous content-proportional algorithm, unchanged.
- Some columns hinted: a hinted column gets `measure * share / 1000`, floored
  at the usual per-column minimum; unhinted columns split the remainder by
  measured width.
- Every column hinted: shares are normalized to 1000 and the rounding leftover
  is shared by the hints.
- Hints that together exceed the available measure are scaled down
  proportionally (900 when unhinted columns still need room, 1000 otherwise).

## Consequences

- `25%` means a quarter of the table, independent of how wide the other
  columns' content is — what an author expects.
- The content floor still applies, so a 1% hint cannot produce a zero-width
  column.
- Word's table grid (`w:tblGrid`) is a natural future source of the same
  field; the docx reader does not set it yet.
