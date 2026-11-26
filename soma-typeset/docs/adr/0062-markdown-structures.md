# ADR-0062: nested lists, GFM tables, and setext headings in Markdown

- Status: Accepted
- Date: 2026-11-21
- Closes: D53
- Related: ADR-0024 (HTML subset), `docs/debt.md` D53
- Touches: `soma-md` only

## Context

The Markdown frontend produced flat lists, no tables, and no setext headings.
Real Chinese manuscripts use all three: nested outlines, comparison tables, and
`标题\n====` written by older editors.

## Decision

1. **Nested lists.** `parse_list` recurses on indentation. A line whose marker
   sits deeper than the current list belongs to the previous item and is parsed
   as a nested `Block::List`, so ordered and unordered levels can alternate.
   Continuation lines are joined into the current item's paragraph.
2. **GFM tables.** A row containing `|` followed by a delimiter row
   (`| --- | :-: | ---: |`) becomes `Block::Table`. Colon placement maps to
   `CellAlign`. `\|` is a literal bar. The first row is a header row. A table
   without a delimiter row stays a paragraph, which is what a reader sees in
   the source.
3. **Setext headings.** A paragraph followed by `===` or `---` becomes a level
   1 or level 2 heading; the underline is consumed. The check runs before the
   thematic-break rule so `标题\n---` is a heading, while a bare `---` line is
   still a rule.

## Consequences

- All three constructs produce ordinary DocIR blocks; no layout or template
  change was needed.
- Cells hold `Block::Paragraph`, so inline parsing, ruby, and index markers
  work inside a cell with no extra code.
- HTML blocks and link reference definitions remain unsupported (D53 keeps
  those two items open in prose).
