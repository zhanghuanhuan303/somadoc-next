# ADR-0084: Table rules own the vertical air, not every cell

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0017 (table grids and headers), ADR-0019 (table rules and
  `within`), ADR-0081 (reference template), ADR-0086 (half-leading)
- Touches: `soma-layout` (table geometry and flow glue),
  `examples/templates/plex.yaml`, `crates/soma-layout/tests/a6_blocks.rs`,
  `e7_tables.rs`, `e8_table_split.rs`, `e9_table_rules.rs`,
  `docs/corpus-calibration.md`

## Context

A comparison against pandoc's `soma.latex` output showed two table defects that
padding alone could not fix:

1. The table's `space-before` and `space-after` from the template were ignored.
   The `FlowItem` used the first row's `space-before` and a zero `after`, so the
   bottom rule almost touched the following paragraph.
2. Giving every cell `0.3 em` of top and bottom padding made body rows much
   taller than the reference. Pandoc/booktabs puts the extra space around the
   **rules** (top, header, bottom), while body rows keep the plain line height.

The reference table measured (300 dpi, Plex at 10pt): 4.8pt between a rule and
the ink, 4.35pt slack between two body lines, and a clear `\parskip` below the
table.

## Decision

1. **Table glue comes from the table element.** The table `FlowItem` uses
   `before_glue`/`after_glue` of the table's computed style, like any other
   block. The first row's `space-before` is no longer repurposed as table glue.
2. **No per-cell vertical padding.** `TableCellLayout.height` is the content
   height; the cell block starts at the row top.
3. **A horizontal rule gets `1/4 em` of space above and below**, in addition to
   its own thickness (`TABLE_RULE_SEP`). The top rule gets the gap below it, the
   header/body rules get both sides, and the bottom rule gets the gap above it.
   The table's own `space-before/after` handles the outside.
4. **The table's top and bottom spacing is template-controlled.** The Plex
   template uses `space-before: 0.3em` (the caption supplies most of the top
   gap) and `space-after: 20pt`, matching the reference's paragraph rhythm.

The geometry is:

```text
top rule | gap | header rows | gap | header rule | gap
         | body rows (plain line height)       |
         | gap | bottom rule
```

## Consequences

- Body rows are compact again; the air sits at the three rules where booktabs
  puts it. At a 10.5pt body the rule gap is 2.7pt.
- The table's bottom rule no longer touches the next paragraph: the template's
  `space-after: 20pt` is applied through the normal vertical glue.
- A table with no rules gets no gaps; grid tables keep their vertical rules.
- `table-page` returns to 2 pages / 125 lines; the extra space now appears as
  bottom slack (809‰), not as row height. `ALL` is 48 pages / 1213 lines.
- Existing tests were updated to the rule geometry; `a6_blocks.rs` asserts the
  gap below the top rule and above the bottom rule.
- No schema change: the geometry is layout output.

## Alternatives

- **Keep cell padding and reduce it.** Body rows stay too tall, and the space is
  in the wrong place (between text and rule on every row); rejected.
- **Use `row` `space-before` as padding.** The first row's value was never
  applied and it is a template policy, not a table baseline; rejected.
- **Hard-code a fixed point gap.** Rejected; the gap must scale with the body
  size like the rest of the table.

## Alignment

- `crates/soma-layout/src/lib.rs` — `TABLE_RULE_SEP`, row/rule geometry,
  `FlowItem` glue.
- `crates/soma-layout/tests/a6_blocks.rs` — rule-gap test.
- `crates/soma-layout/tests/e7_tables.rs`, `e8_table_split.rs`,
  `e9_table_rules.rs` — updated geometry.
- `examples/templates/plex.yaml` — `table` space-before/after.
- `docs/corpus-calibration.md` — new `table-page`/`ALL` baseline.
