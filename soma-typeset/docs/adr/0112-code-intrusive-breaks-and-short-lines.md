# ADR-0112: Long code runs break per character; short lines are priced everywhere

- Status: Accepted
- Date: 2026-11-26
- Closes: D114, D115
- Related: ADR-0111 (inline code at spaces and separators), ADR-0059
  (hyphenation), ADR-0106 (book preset), ADR-0041 (a cost change bumps
  `modelVersion`)
- Touches: `soma-rules` (code segmentation, `Join::Intrusive`), `soma-core`
  (breaker short-line term, `model_version`, `rules_version`), tests, docs

## Context

ADR-0111 let inline code break at its own spaces and after `-`/`/`. Two
boundaries remained:

- **D114**: a code token with no space and no separator — a base64 blob, a
  long hash, a camelCase identifier — was still one atom. A token wider than
  the measure forced an overfull or emergency line, and the line before it
  could not be justified without a hole.
- **D115**: the short-line charge only applied to the paragraph's last line.
  A one-character line in the middle of a paragraph paid nothing extra. The
  normal stage rejects such a line (it has no glue to fill the measure), but
  the emergency ladder can admit one, and then only `badness` priced it.

## Decision

1. **Code separators grow, and camelCase is a seam.** `code_segments` splits
   after `_` always, after `.` between word characters (not between two
   digits, so `1.5` stays one token), and at `someName` / `HTTPServer`
   transitions. Each seam is an ordinary `WORD_BREAK_PENALTY = 500` break.
2. **A long run with no seam is split per character.** A code segment longer
   than `CODE_INTRUSIVE_MIN_CHARS = 12` becomes one atom per character, joined
   by `Join::Intrusive` with `INTRUSIVE_BREAK_PENALTY = 2_000`. The break is
   an ordinary `BoundaryKind::Normal`: no hyphen is drawn and the atoms
   concatenate back to the exact source. The price keeps a token that fits on
   one line whole; the break exists so an oversized token can stay inside the
   measure instead of going overfull.
3. **The short-line term applies to every justified line.** In `eval_line`,
   after trimming and before the boundary-deviation term, a justified line
   pays `short_line_cost(effective, short_last_threshold, short_last_scale)`
   — the same function and the same parameters the last line already used.
   The last line keeps its own `ParagraphEnd` branch. A `ForcedEnd` line is
   exempt: the author asked for that break, so charging it cannot change the
   result. `Line` has no new field; the existing `short_score` in the trace is
   the sum of both uses.
4. **Version bumps.** `model_version` 3 → 4 (the short-line term now covers
   non-final lines) and `rules_version` 1 → 2 (ADR-0111 and this ADR changed
   segmentation and separator penalties).

## Consequences

- A base64/hash token wraps inside the measure; the line before it no longer
  opens a hole. A token that has a case, `_`, `.`, `-` or `/` seam breaks at
  the seam first, because those cost 500 instead of 2 000.
- Code-heavy paragraphs have more atoms (one per character only for long
  unseamed runs). The canonical result gains boxes but the schema is
  unchanged: `short_score` was never serialized, and code atoms already had
  their own boxes.
- `publish-v1`/`book-v1` now price a mid-paragraph one-character line. The
  Chinese corpus baseline is unchanged (`ALL,48,599,1213,0,0,3,0,0,6,809,1404,428,0`),
  because no corpus paragraph needs an emergency short line under the new
  objective.
- `paramHash`, `PagedLayout::hash()` and every canonical result move because
  `modelVersion` and `rulesVersion` are part of the hash tuple. No renderer
  reads those fields, so the SVG golden is unchanged.
- Residual: one glyph wider than the measure still overfulls; that is a
  property of the glyph, not of the break model.

## Alternatives considered

- **Apply Liang patterns inside code spans.** Rejected for this round: the
  patterns live in `soma-layout` and the code span is compiled in
  `soma-rules`; threading them through would make the rules layer depend on a
  pattern file. Per-character breaks solve the overfull case without it.
- **Split long runs into fixed chunks (e.g. four characters).** Rejected: a
  chunk boundary is still a break opportunity, so chunking buys nothing over
  per-character atoms while making the break point coarser.
- **Forbid a one-atom non-final line outright.** Rejected: a paragraph whose
  only atom is wider than the measure would become infeasible. A cost keeps
  the fallback ladder monotone.
- **Add `orphan_scale`/`orphan_threshold` parameters.** Rejected: the
  existing short-line parameters already express the intent, and reusing them
  avoids a parameter-schema change for a term that only the fallback stages
  reach.
