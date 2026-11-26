# ADR-0096: A footnote mark attaches to the text before it; superscripts rise 2/5 em

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0011 (span compiler), ADR-0083 (footnote area), ADR-0085
  (continuous decoration), ADR-0093 (Web Studio)
- Touches: `soma-rules` (span boundary), `soma-layout` (marker span),
  `soma-render-pdf/html/svg`, the R gate, the corpus report

## Context

A footnote reference reaches the layout as an `AtomKind::InlineObject`. The
span compiler gave it a normal boundary, so the CJK/Latin rule inserted a
quarter-em gap before the mark and allowed a break there. In the dense-footnote
sample this put a superscript `1` at the start of a line (the body read
`…多少行` / `¹。当一个…`), which violates the line-start prohibition, and the
gap made the mark look detached from the word it belongs to.

The body mark was also too low. PDF used two thirds of the body size but raised
it only one fifth of an em, so the digit's top sat below the body cap height.
HTML and SVG drew the mark at the full body size on the baseline at all: the
preview did not show a superscript.

## Decision

- **Attachment, not a normal boundary.** `SpanKind::Footnote` compiles to one
  unbreakable object atom whose boundary to the previous atom is
  `Breakability::Forbidden`, with zero glue and `BoundaryRole::Plain`. A line
  therefore cannot start with a footnote mark, and no CJK/Latin gap opens
  before it. The existing `Close`/`Middle` rule still forbids a break before
  the punctuation that follows the mark, so `字¹。` stays one unit.
- **Superscript geometry is 2/3 size, raised 2/5 em.** PDF and SVG draw the
  body mark at two thirds of the block size with its baseline raised by two
  fifths of an em. HTML emits
  `<sup class="footnote-mark">` with `font-size:66.6667%` and
  `vertical-align:0.6em` (0.6 × 2/3 = 2/5 body em), `line-height:0`.
- The footnote-area number keeps the same 2/3 size and now uses the same
  2/5 em raise in PDF and SVG; HTML already raises it because the smaller span
  sits at the line top.
- The marker's advance stays three fifths of a digit (`ObjectMetrics`), so the
  line breaker's width is unchanged.

## Consequences

- Documents with footnotes change line breaks where a mark used to fall at a
  line start: the preceding character moves down with the mark, or the mark now
  fits at the end of the line once the quarter-em gap is gone. The dense
  footnote page no longer starts a line with a superscript.
- Canonical layout hashes and PDF/SVG/HTML bytes change for such documents. No
  schema, parameter or template change.
- Gate R's SVG golden hash is updated, and a new test locks the mark's size and
  raise; a `soma-rules` test locks the forbidden boundary and zero glue.
- Corpus: line and page counts are unchanged. `footnotes` jitter moves
  455‰ → 551‰ because the marker no longer forces a break opportunity; the
  approximation error `bdev_error_p95` drops 9‰ → 0 because marks are no longer
  counted as CJK/Latin boundaries. The aggregate row becomes
  `ALL,48,599,1213,0,0,3,0,0,6,809,1404,428,0`.
- The Studio proof now shows the same superscript as the PDF; `S9`'s ink check
  is unaffected.

## Alternatives

- **Keep a normal boundary but give the marker a zero-width glue.** The break
  opportunity would remain, so the line-start bug would persist.
- **Raise the mark by changing its `baseline_shift` in metrics.** The line box
  would grow and shift baselines; the renderer-only raise keeps the vertical
  rhythm stable, as the existing footnote-area number already did.
- **Use `<span style="vertical-align:super">`.** Browser-defined `super`
  varies between engines; an explicit 0.6em raise on the smaller span is
  deterministic.
