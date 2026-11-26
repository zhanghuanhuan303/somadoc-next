# ADR-0080: A display formula gets a default skip

- Status: Accepted
- Date: 2026-11-24
- Related: ADR-0069 (LaTeX article defaults), ADR-0079 (default math face),
  `docs/math.md` §Pagination
- Touches: `soma-template` (block style resolution), `soma-layout` test,
  `docs/math.md`, `docs/testing.md`

## Context

The built-in `latex-article` template styles `mathBlock` with TeX's display
skips (1em before/after, 0.2em plus, 0.5em minus). A custom template that does
not mention `mathBlock` inherited zero spacing from `ComputedStyle::base`, so
two consecutive display formulas touched: the lower limit of one overlapped the
upper limit of the next. A comparison against the Typst reference
`somadoc-16.pdf` showed the expected breathing room. The template contract says
presentation belongs to the template, but a missing rule should not make
formulas collide; the engine needs a sane lower bound.

## Decision

When resolving a block whose element is `mathBlock`, the base style starts with
one body em before and after, plus 0.2em, minus 0.5em — the same values the
built-in template spells out. Template rules are applied afterwards, so an
explicit `space-before`, `space-after`, `space-before-plus/minus` or
`space-after-plus/minus` overrides each value independently. A template that
sets `space-before: 0` still gets zero.

The spacing is based on the inherited body font size, not the formula's own
size, so it behaves like TeX's `\abovedisplayskip` at the document size.

## Consequences

- Consecutive display formulas no longer touch in a minimal or custom template.
  The reference-style sample and any future template get the expected layout.
- The built-in template is unchanged: its explicit rule sets the same values.
- The corpus page/line counts are unchanged; `math-page` still fits one page and
  the scorecard row is identical (`math-page` jitter 398, `ALL` 427).
- One layout test covers the default: two display blocks in a template without
  a `mathBlock` rule are at least 0.9em apart.

## Alternatives

- **Require every template to set `mathBlock` spacing.** A missing declaration
  then silently produces colliding formulas; rejected. Explicit rules still
  win, so a designer who wants a tight display can set zero.
- **Put the default in the built-in template only.** That is the current state
  and does not help custom templates; rejected.
- **Use a fixed point value.** It would not scale with the body size; rejected.

## Alignment

- `crates/soma-template/src/style.rs` — the `mathBlock` base style.
- `crates/soma-layout/tests/e11_math.rs` — the default-skip test.
- `docs/math.md` — display spacing.
- `docs/testing.md` — test count.
