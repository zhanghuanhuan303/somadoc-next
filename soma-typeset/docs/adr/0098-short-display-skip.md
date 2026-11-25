# ADR-0098: A display after a short line keeps half the template's space-before

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0080 (default display skip), ADR-0026 (display math)
- Touches: `soma-layout` (math item glue), its `e11_math` test, the sample book
  template, `docs/math.md`

## Context

TeX distinguishes a display that follows a short paragraph line from one that
follows a full line: the short form uses `\abovedisplayshortskip` (0pt by
default) and a smaller skip below. The layout copied that rule literally: when
the previous line was short, the template's `mathBlock` space-before was
discarded and the display touched the line above.

Review of the sample book pointed out that a display formula after a one-line
introduction reads cramped. It also contradicts the template contract in
ADR-0080: a template that sets `space-before` expects that value to matter. In
the sample, `book.yaml` sets 2em before/after, yet the formula still started
exactly at the previous block's bottom.

## Decision

When the line before a display is short:

- **Above:** use **half** of the template's `space-before` (width, plus and
  minus), instead of zero.
- **Below:** keep **70%** of the template's `space-after`, as before.

The normal (long-line) path is unchanged. An explicit `space-before: 0` still
resolves to zero, because half of zero is zero. The short ratio is a model
constant, not a new template key; templates keep control through the ordinary
`mathBlock` declarations.

## Consequences

- A formula after a short line now has visible air while still being closer to
  the preceding line than a full-width display. In the sample book (2em
  template skip) the gap above is 1em; the gap below is 1.4em.
- Documents with a display after a short line change their vertical geometry,
  canonical hash and PDF/HTML/SVG positions. Line and page counts change only
  if the extra glue pushes a block across a page boundary; the sample book
  stays at 11 pages.
- The corpus `math-page` fixture may move its formula gap; the Gate Q′ row is
  re-measured rather than treated as a fixed baseline.
- `e11_math` locks the new gap: between 0.49 and 0.61 body em for the default
  1em template skip.

## Alternatives

- **Keep TeX's zero short skip.** Faithful to TeX, but it makes the template's
  explicit spacing ineffective and produced the reviewed result; rejected.
- **Add `math-short-before`/`math-short-after` template keys.** More knobs for
  a rare case, and another thing to calibrate; a fixed ratio keeps the model
  small.
- **Use the full template skip.** Then the short-line rule disappears
  entirely, and a display after a one-line introduction gets as much air as one
  after a full paragraph; too loose.
