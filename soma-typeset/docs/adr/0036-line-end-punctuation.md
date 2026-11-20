# ADR-0036: Line-end punctuation pays only for its ink

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0007 (K-P closure),
  `docs/template.md`, `docs/kp-closure.md`, `docs/params.md`
- Touches: `soma-core` (`eval_line` trim), `soma-rules` (`trim_caps`),
  `soma-layout` (`hanging_amount`), `soma.layout/6` canonical values

## Context

A justified Chinese line ended in `。` looked wrong in the PDF preview: the
period sat half an em short of the right edge. The measurement showed the line
was **not** overfull and the break was correct:

```text
indent 8192, target 120079, natural 118784, adjustment 1295, trimRight 0
last atom x 124175, advance 4096  ->  em-box right edge 128271 = indent + target
```

The em box is flush; the **ink** is not. Chinese closing punctuation is drawn in
the left half of a one-em advance and carries roughly half an em of right side
bearing. Any layout that charges the line for the full advance leaves a visible
half-em hole at every line that ends in punctuation. This is a strategy gap, not
a precision bug (`docs/precision.md` already rules out 1/4096 quantization).

Two strategies were considered.

**Hanging** moves the last em box right, past the text edge, so the glyph lands
on the edge. It was implemented first and rejected: for a full-width mark the
glyph sits at the **left** of its box, so pushing the box right opens the same
half-em hole *between the last two characters*. The hole moves, it does not
close. Hanging belongs to faces whose punctuation is centred in its advance, and
it is kept as an opt-in template knob (`hanging-punctuation`), off by default.

**Compression** (trimming the side bearing) is the native Chinese answer. If the
line only pays for the ink, the last em box is half an em shorter, `adjustment`
grows by the same half em, and the extra glue is distributed between characters
— the ink, not the box, lands on the text edge. This is what TeX's `\rpcode`
does for CJK punctuation, and it is what `Line.trim_right` already modelled; the
only gap was that it fired **only** when a line was overfull.

## Decision

1. **A justified line always drops the trailing side bearing of its last
   atom.** `eval_line` sets `trim_right = atom.compress_right` before computing
   the deficit, for every justified line, underfull or not. `compress_right` is
   the rule table's side bearing for the class and is scaled to the atom's font
   size by `scale_trim`.

2. **A ragged line (paragraph end, forced break) trims only when it would
   overrun.** Its ink then stays inside the block instead of the em box
   pretending to. A ragged line that fits keeps its advance untouched: it has no
   text edge to align to.

3. **Optical alignment is not compensation.** The trim lands in
   `Line.trim_right` and in the ratio/badness through the smaller `effective`
   width, but `compress_cost` is charged only when the line was **originally
   overfull** (`natural > target`). A short line that gives up its bearing is
   aligning, not squeezing.

4. **Leading marks still trim only on overfull lines**, after the trailing
   bearing has been spent. `trim_priority: EndFirst` keeps its meaning.

5. **Half-width punctuation gets a smaller side bearing.** `soma-rules` now
   distinguishes full-width from half-width marks: full-width keeps `1/2em`,
   half-width gets `1/8em`. Before this, a line ending in `,` could be trimmed
   by half an em — more than the comma's whole advance — producing a negative
   `advance` in the canonical output. Full width is `is_cjk(c)` plus the CJK
   quotation marks `“ ” ‘ ’`, which live outside the CJK blocks but are drawn
   one em wide in Chinese faces.

6. **Pair compression only applies to touching marks.** `PAIR_TRIM` is added
   only when the boundary is a plain `Join::Normal`; a space or a forced break
   between two marks ends the pair. Previously `a. (` picked up pair
   compression across the space.

## Consequences

- The ink of every justified line lands on the text edge. Measured on the
  example book: every line's last box right edge is exactly
  `indent + target_width`, and the rasterised ink edge is identical across
  lines to within one pixel.
- The example book keeps 11 pages but its breaks move: a line that ends in
  punctuation now has half an em more room, so it can carry one more character.
  `canonicalHash` moves from `7953d75495e108c2` to `e195431d9622aa4e`.
- `over_compress_lines` on the corpus rises from 0 to 3 of 1213 lines. A line
  that is slightly overfull and ends in punctuation now has a lower badness,
  because it genuinely overruns by less; the model accepts a few of them. The
  5% budget in `compression_past_the_shrink_limit_is_rare` still holds and the
  calibration test documents the new baseline.
- The scorecard's `ratio_jitter_p95` drops 496 → 392: lines that end in a mark
  and lines that do not are now measured against the width they actually
  occupy. The corpus `jitter_p95` stays at 337.
- `soma.layout/6` canonical bytes change (`trimRight`, `adjustment`, the last
  box's `advance` and the downstream `x`s). The schema id does not: the shape
  and the meaning of `trim_right` are unchanged.
- Renderers needed no change: they place glyphs at `boxes[].x`, so a shorter
  `advance` on the last box moves nothing.

## Alternatives considered

- **Hang the mark into the margin** (`min(|adjustment|, cap)`, the pre-ADR
  rule). Rejected as described above: the hole moves to the wrong side of the
  mark. Kept as an opt-in knob for faces where it is correct.
- **Trim only when `natural > target`** (the pre-ADR rule). Rejected: an
  underfull line is exactly the case where the hole is most visible.
- **Change the target width to exclude the bearing** instead of trimming.
  Equivalent geometry, but it would put a per-line fudge into `target_width`
  and make `adjustment` unreadable.

## Adding debt

`punct_class` has no entry for `…` and `—`, so they get no side bearing at all
and cannot trim (D83).
