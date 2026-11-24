# ADR-0082: A justified line trims an opening mark at its start

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0036 (line-end punctuation), ADR-0037 (punctuation ink),
  ADR-0081 (reference template), `docs/kp-model.md`
- Touches: `soma-core` (`eval_line`), `crates/soma-core/tests/k4_trim.rs`,
  `crates/soma-layout/tests/e21_line_end_punctuation.rs`

## Context

In the Plex reference sample a line began with `“见”` after a break. The quote's
ink sat about half an em inside the text edge, visibly out of line with the
paragraph above and below. The cause was the mirror image of the line-end case
in ADR-0036: an opening quote draws its ink inside its one-em advance, and the
breaker only spent the leading bearing when a line was **overfull** (ADR-0036
decision 4). A justified, underfull line therefore kept the full advance and
the ink stayed inset.

The line already pays for the *trailing* bearing of a justified line at all
times (ADR-0036 decision 1). The same reasoning applies at the start: the text
edge is the ink edge, not the em box.

## Decision

1. Under `features.edge_trim`, a **justified** line always trims the first
   atom's `compress_left`, clamped to that atom's advance. This happens after
   the trailing trim, so `trim_priority: EndFirst` still describes the order.
2. A **ragged** line (paragraph end, forced break) keeps the old rule: it trims
   its leading mark only when it would otherwise overrun. It has no right text
   edge to align to.
3. The trim enters `effective` width, `adjustment`, badness and the emitted
   `Line.trim_left` / first box `x`; the extra glue fills the line, so the last
   box still ends at `indent + target`.
4. `compress_cost` is still charged only when the line was originally overfull.
   Optical alignment is not compensation.
5. The rule is class-agnostic: it uses `compress_left`, so an opening quote, an
   opening bracket, or a mark whose outline ink supplies the bearing all work.
   A Han or Latin atom without a bearing has a zero cap and is unchanged.

## Consequences

- A line that starts with an opening mark puts the mark's ink on the text
  edge; the `“见”` line in the reference sample now aligns with the paragraph
  above.
- Break points can move: the underfull line is 0.4–0.5 em narrower before
  justification, so its badness rises. The corpus keeps 0 over-compressed
  lines, 0 widows/orphans and 1213 lines; `jitter_p95` moves 427 → 428.
- A line with no stretch at all can now reach the emergency stage where it
  previously fit exactly. This is the honest cost of optical alignment; such a
  line is already a poor break.
- `soma.layout` canonical bytes change: `trimLeft` and the first box `x`/the
  following `x`s. The schema id stays: the shape and meaning of `trimLeft` are
  unchanged.

## Alternatives

- **Keep leading trim overfull-only.** The original bug; rejected.
- **Hang the opening mark into the margin.** It moves the em box, not the ink,
  and opens a hole between the mark and the next character; rejected (see
  ADR-0036 for the trailing analogue).
- **Exclude the bearing from `target_width`.** Equivalent geometry, but it
  hides a per-line fudge in the target; rejected.

## Alignment

- `crates/soma-core/src/breaker.rs` — the `justified && start + 1 < end` branch.
- `crates/soma-core/tests/k4_trim.rs` — the opening-quote line test.
- ADR-0036 decision 4 is superseded by this ADR; its other decisions stand.
