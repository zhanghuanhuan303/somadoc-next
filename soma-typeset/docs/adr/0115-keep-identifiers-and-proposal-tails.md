# ADR-0115: Keep identifiers whole and price proposal tails harder

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0111/0112 (code breaks), ADR-0106 (book preset),
  ADR-0069 (CLI default), ADR-0114 (default template)
- Touches: `soma-rules` (`WORD_BREAK_PENALTY`, `INTRUSIVE_BREAK_PENALTY`,
  line-start rule), `soma-core` (`ParamSet::proposal_v1`), `soma-engine`
  (preset parser), `soma-studio-core` (fallback preset), `soma-studio-ui`
  (engine label), `examples/templates/proposal.engine`, docs, tests

## Context

A third review of the TACTUS proposal (`soma-studio-8`) claimed no line-breaking
change. Part of the report was an extraction artifact: `pdftotext` emits an
inline code run on its own line because it has a different font and size, so
`metronome-rs` looked "floating"; the rendered page has it inline on the line
"基于 `metronome-rs` 的代码与架构…". The `tactus-time` lines are list items.

Two real weaknesses remained:

- A word/identifier break cost `500`, which is cheaper than a very loose line's
  badness. In the book sample, `soma.render/7` was split after the dot even
  though the token would have fit on the next line. The review's ask —
  "`[a-zA-Z0-9_-]+` keep-together" — needs a price outside the normal search.
- `book-v1` charges a last line only below 3 em. A 5–8 character Chinese tail
  pays nothing, and the TACTUS pages show such tails ("时间轴。" is 4 em).

The review also showed inline code starting a line with `*` (`* 60 * …`), which
reads as a stray operator rather than a continuation.

## Decision

1. **A separator break is a last resort.** `WORD_BREAK_PENALTY` rises from 500
   to 3000 and `INTRUSIVE_BREAK_PENALTY` from 2000 to 8000. Badness at the
   tolerance ceiling is a few hundred, so identifiers, crate names and URLs stay
   whole unless the token itself is wider than the line. This is still a price,
   not a hard rule: an over-wide token can always wrap.
2. **`proposal-v1`.** A new preset is `book-v1` with
   `short_last_threshold = 8em` and `short_last_scale = 2_000_000`: a 5–8
   character tail is charged, and at twice the book rate. The Studio uses it as
   its fallback preset and pins it in `examples/templates/proposal.engine`; the
   command line keeps `latex-v1` (ADR-0069). `book-v1` is unchanged for books.
3. **Operators do not start a line.** A boundary before `*`, `+` or `=` is
   forbidden, like the existing `-` and `/`. A wrapped expression ends the line
   with the operator (`tick *`) and continues below.

## Consequences

- Identifiers split less; the trade-off is a slightly looser or longer line
  when the whole token cannot fit. The corpus baseline (publish-v1) is measured
  and unchanged: no corpus paragraph depends on a 500-cost separator break.
- The Studio's default output changes: 5–8 character tails are now charged at
  2M. `book-v1` and `latex-v1` keep their output.
- A line can no longer begin with an operator in code; prose is unaffected
  because a spaced operator is a space boundary, which has no such rule.
- `paramHash` moves for `proposal-v1` only.

## Alternatives considered

- **Hard keep-together groups in the DP.** Rejected for this round: the breaker
  has no token state, and a hard rule would make a token wider than the measure
  infeasible. A high price gets the same result wherever an alternative exists.
- **Forbid separator breaks in the normal stage.** Rejected: a URL or crate name
  wider than the measure would have to reach the emergency ladder, which looks
  worse than a clean break after `/`.
- **Raise `short_last_threshold` beyond 8 em.** Rejected: the validator caps the
  term at 8 em and a longer threshold starts charging ordinary last lines.
- **Change `book-v1` in place.** Rejected: presets are data with pinned ids; a
  new preset records the proposal-specific choice without changing books.
