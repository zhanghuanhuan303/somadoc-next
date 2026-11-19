# ADR-0020: Table column rules, rule styles, and colors

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0019 (table rules and `within`), `docs/debt.md` D68
- Extends: the template `table:` section (`soma.template/4`), the table branch
  of `soma-layout`, and both renderers.

## Context

ADR-0019 drew only horizontal rules, always solid, always `#333`. A full grid
and the common "单元格边框" appearance need vertical rules; printed tables also
use dashed guides and colored rules. The three horizontal widths could not
express a left/right frame independently from the top/bottom rules.

## Decision

### Five independent widths

```yaml
table:
  rule-width: 0.4pt               # horizontal, between body rows
  header-rule-width: 0.6pt        # below the header
  outer-rule-width: 0.8pt         # top and bottom
  column-rule-width: 0.4pt        # vertical, between columns
  outer-column-rule-width: 0.8pt  # left and right frame
```

`rule-style: solid | dashed` and `rule-color: "#rrggbb"` apply to every rule.
Color defaults to the document color (renderers fall back to `#333`); the rule
color is interned once as a `StyleId` on `TableRules`, so renderers keep reading
only `StyleTable`.

`column-rule-width` plus the five horizontal widths is the complete cell-border
model: every internal boundary and the frame. Spanning cells suppress the
segment they cover (rowspans for horizontal rules, colspans for vertical ones),
so a spanning cell never has a line through it.

### Vertical geometry

A vertical rule at boundary `c` is centered on the column boundary (`x =
c × column_width − w/2`); the left frame occupies `[0, w)` and the right frame
`[width − w, width)`. The rule is split into uncovered y-intervals by the cells
that span `c`, then clipped again at the header height so the repeated header
carries its own side lines: the `[0, header)` part of a vertical rule ends
exactly at the header boundary and is therefore copied into `header_blocks`.

### Dashes are geometry, not paint

`rule-style: dashed` makes the layout emit multiple `TableRule` blocks with a
fixed pattern: dash = `3 × thickness`, gap = `2 × thickness`, measured from the
start of each segment (both directions), with the last dash clipped to the
segment. The renderer draws rectangles and never learns about dash semantics, so
the result stays deterministic and part of the page hash.

## Consequences

- `soma.template/4` adds three keys to `table:`; `soma.pages/4` is unchanged
  (no new block kind, only more `TableRule` instances).
- D68 closes: color, dashed rules, vertical rules and cell borders are all
  expressible. Per-table/per-row overrides are still not possible (the
  template's `table:` section is global); that remains a smaller debt.
- A dashed long table produces many blocks; this is proportional to line length
  divided by `5 × thickness`, and is the price of keeping dashes in layout.

## Alternatives rejected

- **Dash in the renderer (CSS/SVG `stroke-dasharray`).** The HTML and SVG
  backends would compute the pattern twice and it would not be part of the
  deterministic layout hash.
- **A border property per cell.** Cell borders are exactly the grid of boundary
  rules; per-cell borders would duplicate the same geometry and need DocIR
  support for cell decoration.
- **One width for top/bottom and left/right.** Three-line tables use a heavy
  outer frame with no side lines, so the horizontal and vertical outer widths
  must stay independent.
