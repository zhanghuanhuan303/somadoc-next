# ADR-0111: Inline code breaks at its spaces and separators

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0011 (span compiler), ADR-0059 (hyphenation),
  ADR-0069 (CLI keeps the LaTeX default), ADR-0105 (line trim),
  ADR-0106 (book preset)
- Touches: `soma-rules` (`SpanKind::Code`, `Join`, `normal_boundary`),
  `soma-studio-core` (fallback preset), `soma-studio-ui` (read-only engine
  label), `docs/studio.md`, tests

## Context

A third-party review of the TACTUS proposal PDF (rendered by the Studio) put
line breaking at 6.0–6.5 and named four symptoms:

1. a long inline-code expression such as
   `round(tick * tempo_num / (tempo_den * ppq))` was one atom, so it could not
   wrap: the line either overflowed or pushed a large hole into the line before
   it;
2. identifiers like `metronome-rs` and `tactus-time` were cut at the hyphen
   immediately (the boundary had penalty 0), and the tail could end up alone on
   the next line;
3. short last lines (orphans) were frequent;
4. the grey of the paragraph was uneven around those long atoms.

Two causes were mechanical rather than a matter of taste:

- `SpanKind::Code` compiled to exactly one atom in `soma-rules`, so the breaker
  had no legal break inside a long expression;
- `push_text` ended a Latin run after `-` or `/` and left the following
  boundary at `Join::Normal`, i.e. a free break.

The short-last-line symptom was a preset mismatch: the Studio fell back to
`ParamSet::latex_v1()`, which deliberately sets `short_last_scale = 0` for
LaTeX compatibility, while `book-v1` already charges a short last line from
3 em (ADR-0106).

## Decision

1. **Inline code is split at its own whitespace and, at a price, at its
   separators.** `paragraph_from_spans` splits a code span into pieces that
   each end at a run of whitespace; the whitespace stays at the tail of the
   piece before it, so concatenating the atoms reproduces the exact source.
   Each piece is then split after `-` or `/` when a non-space follows
   (a separator next to a space is left to the space, which is the better
   break). A boundary at a code space is `BoundaryKind::Normal`,
   `BoundaryRole::Plain`, zero glue, and `CODE_BREAK_PENALTY = 100`: it is a
   real break opportunity, but a last resort like a hyphenation point. A
   separator boundary inside a piece uses the same shape with
   `WORD_BREAK_PENALTY = 500` (decision 2). A code run with neither a space
   nor a separator (`a1b2c3`) stays one atom.
2. **A Latin run keeps its separator.** A boundary after `-` or `/` is
   `WORD_BREAK_PENALTY = 500` with the ordinary mixed-script glue and the
   ordinary forbidden checks. `/` now belongs to the Latin run, so `time/core`
   becomes `time/` + `core` instead of `time` + a stray `/` + `core`; a
   standalone `-`/`/` also opens a priced boundary. A long URL can still wrap;
   a word that fits stays whole.
3. **Kinsoku is independent of a break opportunity.** A boundary before an
   atom that starts with `-` or `/`, or before closing/middle/dash
   punctuation, is forbidden even when the previous atom ends at a break
   opportunity. Before this change `well-，` could start a line with the comma.
4. **The Studio defaults to `book-v1`.** `soma-studio-core::build` uses
   `ParamSet::book_v1()` when no `--engine` is given; the command-line tool
   keeps `latex-v1` (ADR-0069). The Studio deployment also pins
   `--engine examples/book/book.engine`, so the running server does not depend
   on a future default change.

## Consequences

- A long inline-code expression wraps at its spaces instead of overflowing; a
  line containing inline code no longer stretches the surrounding spaces to
  absorb an unbreakable atom. A long URL wraps after a path separator.
- A break inside a hyphenated identifier is possible but priced. It is chosen
  only when the alternative is overfull or a much looser line, and no
  discretionary hyphen is drawn because the separator is already in the text.
- Inline code is no longer a single atom in `Paragraph::atoms`. Consumers that
  assumed one atom per code span must iterate instead: the test
  `code_and_object_spans_are_single_atoms` was replaced by
  `inline_code_breaks_only_at_its_own_spaces`, and the reconstruction contract
  is covered by `e28_code_break`.
- The corpus baseline is unchanged
  (`ALL,48,599,1213,0,0,3,0,0,6,809,1404,428,0`): the corpus has no inline code
  with spaces and no identifier break that the new penalty would move.
- The Studio's visible preset changes from `latex-v1` to `book-v1`; the change
  is pinned by `the_studio_defaults_to_the_chinese_book_preset`.
- Remaining gaps are recorded in `docs/debt.md` as D114 (a code run with no
  space and no separator still cannot break, and Liang patterns apply to text
  spans only) and D115 (a one-character line other than the last is not
  charged).

## Alternatives considered

- **Give inline code zero-cost breaks at spaces.** Rejected: code breaks are
  visually louder than word spaces, and a free break would let the breaker cut
  an expression to tidy up an unrelated line.
- **Draw a discretionary hyphen at a `-` break.** Rejected: the hyphen is
  already in the text; drawing another would print `metronome--rs`.
- **Enable `book-v1` by changing the CLI default.** Rejected: ADR-0069 freezes
  `latex-v1` as the command-line default for LaTeX compatibility. The Studio is
  a separate product surface with its own preset.
- **Model a line-initial `/` as a new punctuation class.** Rejected: the class
  table is about trimming and full-width behaviour; a separator is a
  break-opportunity concern and belongs in the boundary rule.
