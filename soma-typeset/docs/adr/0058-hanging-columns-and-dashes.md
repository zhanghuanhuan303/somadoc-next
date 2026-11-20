# ADR-0058: hanging indents, content-sized columns, wide dashes, and table cells that are inlines

- Status: Accepted
- Date: 2026-11-20
- Closes: D83, D90, D89, and the LaTeX half of D70/D54
- Related: ADR-0007 (frozen kernel), ADR-0057 (lists and container indents),
  `docs/debt.md` D70, D83, D89, D90
- Touches: `soma-core` (`ParagraphStyle`, breaker), `soma-rules` (`PunctClass`,
  `punct_class`), `soma-latex` (cells, `\item[label]`, `description`),
  `soma-layout` (table columns, hanging mapping). `soma.layout/6` keeps its
  shape: `Line.indent` can now be negative, which the schema already allowed.

## Context

Four gaps from the same list, all of them visible in the delivery brief:

1. **Columns were equal slices of the measure** (D90). A two-column table of
   "序号 / 说明" gave each half the page; a long Chinese sentence wrapped into a
   column two characters wide.
2. **Lists could not hang** (D89). The marker sat at the item's left edge and
   wrapped lines did too, because `Paragraph::validate` refuses a negative
   `first_indent` and the kernel had no other way to widen a first line.
3. **`…`, `—`, `–`, `·` had no class** (D83). They were `PunctClass::None`, so
   they could start a line and had no side space to give up; classifying them
   as `Middle` was worse, because `Middle` trims half an em from each side and
   would close up a `——` pair.
4. **A table cell was one literal text run** (D70). `\textbf{阶段}` printed as
   `{阶段}` — the backslash disappeared and the braces stayed.

## Decision

### 1. `ParagraphStyle::hang` — the kernel's half of a hanging indent

```rust
pub struct ParagraphStyle {
    pub width: Width,
    pub first_indent: Width,  // inward, first line only
    pub hang: Width,          // outward, first line only
    ...
}
```

`target(0)` becomes `width + hang - first_indent`; line 0's `indent` becomes
`first_indent - hang`; every later line is unchanged. `hang` defaults to zero,
so every existing paragraph keeps byte-identical output, and the DP's cost
model does not change — only the measure of the first line.

`first_indent` and `hang` are never both non-zero: a line is either indented
inward (a normal paragraph) or outward (a marker in the margin).

The template keeps writing what an author expects: a **negative**
`first-indent` is a hanging indent. `soma-layout` splits it into
`hang = -first-indent, first_indent = 0` when it builds the paragraph, so the
template language does not need a second property and the kernel does not need
to accept negative indents.

`Line.indent` can therefore be negative, which the canonical schema already
allows (it is an `i32`).

### 2. Table columns are sized from their contents

`table_item` measures each cell's widest unbroken line (`natural_width`, which
reuses the span compiler and the object metrics), takes the max per column,
distributes a colspan cell's demand across the columns it covers, and then
allocates:

- every column keeps a floor of `measure / 6n`;
- the rest is shared in proportion to natural width;
- largest-remainder rounding makes the widths sum to exactly the measure,
  which the rules and span offsets depend on.

A table of short cells still fills the measure (the shares scale up); a table
with one long column gives that column the room.

### 3. Wide marks get their own class

`PunctClass::Dash` — ellipsis, em dash, en dash, middle dot. Break rules treat
it like `Middle` (no line may start with it), but trimming is zero on both
sides and it never hangs. `——` stays two em wide.

### 4. A cell is inline content

`parse_tabular` compiles each cell with `parse_inlines`, so emphasis, code and
`$…$` maths work inside a table, and footnotes in a cell are lifted into the
document. `\item[Term]` and `description` become a bold run-in label on the
item's first paragraph — the item model has no separate term box, and without a
hanging indent (until this ADR) a term column could not have been expressed
anyway.

## Consequences

- The brief's tables are readable: `指标 / 数值` no longer splits the page in
  half, and the sample book's `阶段 / 触发条件 / 可见后果` gives the short
  column a short column's width.
- List items hang: the bullet sits 1.2 em left of the text column, and every
  wrapped line starts at the text column.
- `\textbf{阶段}` in a table cell prints as bold `阶段`.
- Corpus: line count and fitness jumps are unchanged; `jitter_p95` moves
  slightly because dashes and ellipses are no longer shortened.
- Tests 629 → 633: hanging geometry, column allocation, dash class and trim,
  cell inlines with a label, inline-code advance.

## Alternatives

- **Let `first_indent` go negative in the kernel.** `validate` rejects it for a
  reason: every consumer of `first_indent` treats it as an inward offset. A
  separate `hang` says what the geometry means.
- **Implement hanging by shifting line 0 after breaking.** The line would then
  be `hang` narrower than its measure, so justified text would show a ragged
  first line — the very thing a hanging indent is for.
- **Equal columns with a manual override.** A template-only `widths:` list
  forces every author to count characters; measuring is what the engine is for.
- **Trim dashes like a comma.** `Middle`'s half-em trim would close `——` to
  one em and pull `……` together; the marks carry their space themselves.
