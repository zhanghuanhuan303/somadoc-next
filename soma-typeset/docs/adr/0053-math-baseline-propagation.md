# ADR-0053: every math node places itself at the baseline it is given

- Status: Accepted
- Date: 2026-11-20
- Closes: D88
- Related: ADR-0026 (math box model), ADR-0030 (math completion),
  `docs/debt.md` D88
- Touches: `soma-math` only. No schema change; the page result keeps its shape.

## Context

A display fraction printed wrongly: the bar was invisible, the numerator sat on
top of it, and the denominator overlapped the numerator. A radical did the
same — its surd crossed its own radicand. Both had been true since the box
model landed, and both went unnoticed because the shipped samples only used
inline formulas without fractions.

`Context::node(node, size, baseline)` is supposed to place a subtree so that its
own baseline is `baseline`. `Symbol`, `Group`, `Script` and `Fraction` honour
that. Three arms did not:

* `Radical` pushed its surd at `thickness`, its body at `0`, and its bar at
  `-needed` — all relative to the *formula's* baseline. Inside a fraction the
  numerator therefore collapsed onto the bar.
* `Delimited` placed its body at `0` and its fences at `baseline_off`, ignoring
  the enclosing baseline the same way.
* `Matrix` did the same for its cells, its row baselines and its fences.

The fraction had a second, independent bug: it cleared the bar with the
numerator's *height* and the denominator's *depth*. Those are the wrong edges.
What must not touch the rule is the numerator's bottom (`depth`) and the
denominator's top (`height`); using height for the numerator lifted it by its
whole cap height, and using depth for the denominator dropped it by a quarter
em, so the two overlapped no matter what the bar did.

## Decision

### 1. A node offsets its children by the baseline it receives

`Radical`, `Delimited` and `Matrix` add their `baseline` to every `y` they emit
— surd, radicand, radical bar, fences, matrix cells, matrix fences. `matrix()`
takes the baseline as a parameter so its row baselines are computed relative to
the node's own baseline rather than the formula's.

### 2. A fraction clears the bar by the edges that face it

```rust
let numerator_baseline   = rule_y - gap - numerator_box.2;   // depth
let denominator_baseline = rule_y + thickness + gap + denominator_box.1; // height
```

Both gaps are then exactly `frac_gap`, independent of the glyphs: a fraction of
two capital letters and a fraction of two commas both clear the rule by the
same amount.

## Consequences

- `\frac{\sqrt{\pi}}{2}` renders as three separate bands: the radical bar over
  `π`, the fraction bar, and `2` below it. The regression test checks the order
  of those `y` values, so the bug cannot come back silently.
- `\left( \begin{matrix} … \end{matrix} \right)` inside a fraction or a
  superscript now lands at the right height instead of at the formula baseline.
- `examples/book/song.tex` sets a fraction and a radical on the same page as a
  sum, so the sample exercises all three.
- The math block gets `space-before`/`space-after` in the sample template: a
  display formula with stacked limits needs air above and below, and the
  defaults are zero.

## What is still not TeX

- **Variables are upright.** The math face has no italic, so `d` in `dx` is
  roman. For differentials that matches ISO 31/11; for variables it does not.
  Fixing it needs an italic math face and a `\mathit` distinction in the parser.
- **No `\text{}` inside math**, no operator spacing table beyond bin/rel/punct,
  no `\big` delimiters. All recorded under D71.

## Alternatives

- **Offset the items after the fact** (walk the slice and add the baseline).
  Works, and hides which node forgot: the next node added would forget again.
  Passing the baseline down is the invariant, and it is now true of every arm.
- **Give `Fraction` its own placement path and let radicals stay absolute.**
  That is what the code did, and it is why a radical numerator broke. A nested
  node must not have to know it is nested.
