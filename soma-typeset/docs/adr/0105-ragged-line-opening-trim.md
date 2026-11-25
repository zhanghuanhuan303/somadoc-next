# ADR-0105: A ragged or left-aligned line also trims an opening mark at its start

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0036 (line-end punctuation), ADR-0082 (justified line-start trim),
  ADR-0038 (glyph edges), ADR-0104 (mixed syntax)
- Touches: `soma-core` (`breaker.rs`), `soma-layout` (`align_lines`),
  `crates/soma-core/tests/k4_trim.rs`,
  `crates/soma-layout/tests/e14_alignment.rs`

## Context

ADR-0082 made a **justified** line trim the left side bearing of its first
mark. A ragged line (paragraph end, forced break) or a left-aligned block
(headings, list-like text, HTML blocks with `align: left`) still kept the full
advance. A heading such as `“数字花园”` therefore put the quote's ink about
0.58 em inside the text edge while the body and a plain heading started on the
edge. The user reported exactly this: the double quote at the line start looked
inward against the rest of the page.

The right edge rule from ADR-0036 is separate: a ragged line has no right text
edge to align to, so it must not trim its trailing mark unless it would overrun.
The **left** text edge, however, exists for every left-aligned line.

## Decision

1. Under `features.edge_trim`, a line with at least two atoms whose first atom
   has `PunctClass::Open` trims that atom's full `compress_left`, even when the
   line is ragged or the paragraph is left-aligned. The opening mark's ink lands
   on the left text edge.
2. Only opening marks get the full optical trim on a ragged line. Other first
   glyphs keep their natural position, so Latin headings and ordinary leading
   Han characters do not shift by their side bearing.
3. Trailing trim for a ragged/left line is unchanged: it still happens only when
   the line would overrun (ADR-0036). An overfull line still spends end trim
   first and then the minimum start trim needed to fit (ADR-0082).
4. The trim is still `Line.trim_left`; the boxes start at
   `indent - trim_left`. On a left-aligned line there is no justification glue
   to absorb it, so the line's em boxes move left together and the opening mark
   hangs into the margin. That is the intended optical result: the first ink is
   on the edge, the following text keeps its normal relationship to the quote.
5. Centering and right alignment are applied after the kernel. Because the
   kernel has only Justify/Left, `soma-layout::align_lines` undoes a purely
   optical leading trim on a centered/right line when `natural <= target`, then
   centres or flushes the unchanged natural width. A genuinely overfull line
   keeps its trim.
6. No schema changes: the meaning of `trimLeft` is still "how far the line's
   start is trimmed"; only the set of lines that use it grows.

## Consequences

- A heading or left-aligned line beginning with `“`, `‘`, `（`, `「`, etc. now
  puts its ink on the same left edge as the body. The `“数字花园”` case aligns.
- Justified body lines keep the ADR-0082 geometry; the corpus line count,
  compression, widow/orphan and jitter baselines are unchanged.
- Lines that begin with an opening mark change canonical bytes (`trimLeft` and
  the box coordinates). The `soma.layout`/`soma.pages` schema ids do not change.
- A centered title beginning with a quote keeps its previous centered geometry;
  only left-aligned text gets the optical start trim.
- The trim is a line-start shift, not a glyph-only hang. A future "hang the ink
  but keep the advance" model would need a separate field and renderer support;
  the current choice matches the existing `trimLeft` contract and every backend.

## Alternatives

- **Keep ragged lines untouched.** Rejected: it leaves the reported heading
  inset and contradicts the left-edge alignment already used by justified
  lines.
- **Apply the full optical trim to every first glyph, not only opening marks.**
  Rejected for ragged lines: Latin headings would shift by their side bearings
  and the left edge would become visibly ragged. Justified lines keep the
  class-agnostic ADR-0082 rule.
- **Add a glyph-only offset field.** More precise for a hanging quote, but it
  changes the line/box schema and all three renderers. Deferred until a real
  need for a hanging start mark appears.
- **Detect headings in the kernel.** The kernel does not know block kinds;
  `PunctClass::Open` plus `align` is the available seam.

## Alignment

- `crates/soma-core/src/breaker.rs` — the leading `PunctClass::Open` branch and
  the unchanged trailing-trim condition.
- `crates/soma-layout/src/lib.rs` — `align_lines` undoes optical trim for
  centered/right paragraphs.
- `crates/soma-core/tests/k4_trim.rs` — ragged opening quote, ragged non-mark.
- `crates/soma-layout/tests/e14_alignment.rs` — centered quote keeps centring.
