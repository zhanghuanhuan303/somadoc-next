# ADR-0057: a container indents its children; a list item carries its marker

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0009 (semantic/presentation boundary), ADR-0011 (inline span
  compiler), ADR-0031/0032 (templates and generated content), `docs/debt.md` D89
- Touches: `soma-layout` (`flow_into`, `paragraph_item`), `soma-latex`
  (`parse_blocks`), the sample template. No schema change.

## Context

While setting the delivery brief, three things turned out not to work:

1. **Lists had no markers.** `Block::List` was flattened into its items'
   paragraphs, so `\item` printed as an indented paragraph with nothing in
   front of it. Markdown, HTML and LaTeX all produce lists; the sample had been
   shipping a "list" that was really six paragraphs.
2. **Containers had no geometry.** `list`, `listItem` and `blockQuote` carried
   `indent-left`/`indent-right` in the template, and nothing read them: a
   paragraph inside a quote was laid out exactly like a body paragraph. Style
   inheritance (`ComputedStyle::inherited_from`) deliberately copies only
   font-ish properties, so box properties stopped at the element that declared
   them — and the element that declares them is the container, which emits no
   box of its own.
3. **A paragraph that started with an inline command was split.** In
   `parse_blocks`, any command other than `begin`/`item`/`section`/… fell into
   the "unknown block command" arm, which reads the command plus one braced
   group and emits it as its own paragraph. `\item \textbf{甲}：正文` therefore
   produced two paragraphs, and the same happened to any block starting with
   `\emph`, `\texttt`, `\href`, …

## Decision

### 1. `flow_into` carries an accumulated indent

`Builder` keeps `extra_left`/`extra_right`. A `BlockQuote` adds its own
`indent-left`/`indent-right` around the recursion into its blocks; a `List` adds
its own plus each item's `listItem` indent. `paragraph_item` adds the
accumulation to the block's own indent, shrinks the measure by the sum, and
places the box at the accumulated left offset. Restoring the accumulator after
the recursion keeps siblings unaffected, and nested quotes/lists add up.

This keeps the frozen kernel out of it: `ParagraphStyle` still has one width and
one (non-negative) first-line indent; only the geometry the layout hands it
changes.

### 2. The marker is text

An unordered item's first paragraph is prefixed with `•` (U+2022), an ordered
one with `1.`, `2.`, … starting at `List::start`. The marker is an ordinary
inline run, so it takes part in line breaking and justification like any other
text, and the first paragraph of an item is laid out once, not twice.

Consequences of that choice, honestly:

- **No hanging indent.** A true hanging indent puts the marker in the margin and
  aligns the text after it. That needs a *negative* `first-indent`, which
  `Paragraph::validate` refuses — the kernel's model is "the first line is
  indented inward". Today the item is indented as a whole (`list` + `listItem`)
  and its paragraphs declare `first-indent: 0`, so every line of an item starts
  at the same place. Recorded as D89.
- **The bullet is full-width.** In a CJK face U+2022 is a 1 em glyph, so the gap
  between bullet and text is wider than a Latin bullet's. It reads as Chinese
  list typography, which is what it is.
- **A marker stretches with justification.** The space after it is an ordinary
  space boundary and takes the line's adjustment. A short first line therefore
  shows a slightly wider gap after the bullet.

### 3. Inline commands are not block commands

`parse_blocks` asks `starts_inline_run(command)` before taking the
"unknown block command" arm. `\textbf`, `\emph`, `\textit`, `\textsl`,
`\underline`, `\textsc`, `\textsuperscript`, `\textsubscript`, `\texttt`,
`\verb` and `\href` fall through to `parse_inlines`, which already knows how to
read them. Any other unknown command keeps the old recovery behaviour.

## Consequences

- The sample's "structure list" now shows bullets, and the quotation in the
  brief is indented on both sides — both were declared in the template and both
  were dead before.
- `docs/corpus-calibration.md`'s jitter figure moved: the corpus template also
  declares quote/list indents, so the corpus now lays out with them. Current
  baseline is `ALL,48,599,1213,0,0,3,0,0,3,486,1580,377,9` — same line count,
  same zero compress/emergency, `fitness_jumps` still 3, jitter p95 368 → 377.
  The calibration decisions recorded in ADR-0042 and `docs/trim-calibration.md`
  are about trim caps and are unaffected.
- Three new layout tests (bullet marker and indent, ordered start, quote
  indent) and one LaTeX test (an item opening with `\textbf` stays one
  paragraph).

## Alternatives

- **Make `inherited_from` copy `indent_left`.** One line, and wrong: a
  `list` (2 em) followed by a `listItem` (1.2 em) would give the paragraph
  1.2 em instead of 3.2 em, because inheritance overrides rather than adds.
  Containers need to *contribute*, which is a layout concern.
- **Emit the marker as its own box.** It would have to share the first line's
  baseline with the text box, which the flow model does not express: a flow item
  is a block, not a line fragment. Putting the marker in the text keeps one
  box, one break pass, one line.
- **Teach the kernel negative indents.** That is the frozen model (ADR-0007);
  a hanging indent is a real feature request, not a parameter.
