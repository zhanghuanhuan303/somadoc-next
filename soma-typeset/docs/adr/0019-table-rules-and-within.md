# ADR-0019: Table rules and `within` selectors

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0003 (pure YAML template), ADR-0009 (semantics/presentation
  boundary), ADR-0017/0018 (table grids and continuation), `docs/debt.md`
  D47/D51
- Extends: the template schema (`soma.template/3`), the table branch of
  `soma-layout`, and both renderers. Page schema becomes `soma.pages/4`.

## Context

After ADR-0018 a table could continue across pages, but it was still a flat
grid of text with no lines, and a template could not style the paragraphs
inside a cell: `element: paragraph` applies to every paragraph, and DocIR has
no "paragraph inside a cell" element. The last corpus lesson (cell paragraphs
inheriting body presentation) had no template-level answer.

Two presentation needs were therefore missing:

1. three-line (三线表) and full-grid rules, positioned in the vertical flow;
2. a selector that says "this rule only applies to paragraphs inside a cell"
   without inventing new DocIR nodes.

## Decision

### Rule widths live in a template-level `table:` section

```yaml
table:
  rule-width: 0.4pt          # between body rows
  header-rule-width: 0.6pt   # below the header rows
  outer-rule-width: 0.8pt    # above row 0 and below the last row
```

`TableSpec` is resolved against the root font size into `TableRules`
(1/4096 em) and stored on `StyledDoc`. Every value defaults to zero, and a
zero value draws nothing, so templates without a `table:` section produce
exactly the previous geometry. The three widths are independent on purpose:
a three-line table sets `rule-width: 0` and non-zero header/outer widths; a
full grid sets all three.

The alternatives (making rule width a `ComputedStyle` property, or a per-row
declaration) were rejected for this step: a style property would change every
`styles` hash and would not say which boundary it applies to, and per-row
overrides need a row selector that DocIR does not have.

### Rules are positioned blocks, not borders

Each rule is a `PlacedBlock` of the new kind `TableRule` with exact `x`, `y`,
`width`, `height`. Rules occupy real vertical space: `row_top[r]` includes the
top rule and the rule after each previous row. A continuation page therefore
shifts rules like any other block, and the repeated header carries the top
rule and the header rule because `header_blocks` is now "every block that ends
above the header height".

`header_reserve` only reserves when `header_rows > 0`, so a headerless table
with outer rules does not lose space on continuation pages.

A rule never crosses a spanning cell: for each boundary the layout marks the
columns covered by a cell that spans it and emits one rectangle per contiguous
uncovered run. A spanning cell keeps its rows whole, as in ADR-0018.

Rules are painted with the renderer's fixed rule color (`#333`, the same as
thematic breaks). Color is not part of this template schema; that is recorded
as debt.

### `within` adds one ancestor constraint to selectors

```yaml
- selector:
    element: paragraph
    within: cell
  declarations:
    first-indent: 0
    indent-left: 0.25em
```

`Selector::matches` takes the element name of the block ancestors collected
during resolution. `within` matches when any ancestor has that element name.
Specificity places `within` with `role` (below classes, above element).

The resolver now walks the document as a stack (`table` → `row` → `cell` →
`paragraph` → inlines, `list` → `listItem`, `footnote` → blocks), so the same
mechanism works for any future context, not just cells. Because `within` is a
selector condition and not inheritance, layout properties such as
`indent-left` can be turned into cell padding without changing what body
paragraphs inherit.

This supersedes the "no combinators" limitation of D47 for the single-ancestor
case; deeper descendant chains, child combinators and pseudo-classes remain
out of scope.

## Consequences

- `soma.template/3` adds `table` and `within`; `soma.styles/2` is unchanged
  (no new `ComputedStyle` fields).
- `soma.pages/4` adds the `tableRule` block kind. Documents and templates that
  do not use rules keep byte-identical page output except for the schema
  string.
- The table fixture in the score template uses a three-line table, so the
  corpus exercises rules end to end (table-page bottom slack 361‰ → 278‰).
- D51 narrows to nested boxes and vertical column rules; D47 narrows to full
  combinator support.

## Alternatives rejected

- **Raise `ComputedStyle` with border properties.** Every style hash changes,
  and borders still need per-boundary semantics (header vs body vs outer).
- **Draw rules in the renderer from cell coordinates.** The renderer would
  have to reconstruct row boundaries, column spans and page cuts; rules would
  no longer be part of the deterministic layout hash.
- **Emit one full-width rule from column 0 to the table width even inside a
  span.** It would strike through spanning cells; per-column segments cost one
  coverage vector per boundary and avoid that.
