# ADR-0059: hyphenation as a boundary kind and a line property

- Status: Accepted
- Date: 2026-11-21
- Closes: D09
- Related: ADR-0007 (frozen kernel), `docs/debt.md` D09, D60
- Touches: `soma-core` (`BoundaryKind`, `Line`, breaker), `soma-rules`
  (`SpanKind`, `Join`), `soma-layout` (span splitting, `layout_with_hyphenation`),
  `soma-cli` (`--hyphenation`), `soma-hyphen` (new crate),
  renderers (one hyphen per broken line)

## Context

A long Latin word or a URL had no legal break inside it. The engine could only
break after a literal `-` or `/` (E6), so a word wider than the measure produced
an overfull line or an emergency line. A Chinese-first book still contains
English terms and URLs, and the corpus shows this as the last remaining source
of overfull lines.

TeX solves this by interleaving discretionary hyphens into the word, and by
charging the hyphen's width to the line that breaks there. Liang's pattern
algorithm decides where the discretionary points are.

## Decision

### 1. Patterns are host data, not a built-in dictionary

`soma-hyphen` implements Liang's algorithm and parses a TeX pattern file. It
ships **no** patterns: a dictionary has its own licence and the engine should
not choose a language for the document. The caller passes a pattern set:

```rust
layout_with_hyphenation(doc, styled, metrics, engine, strategy, vertical, Some(&patterns))
```

`layout()` and `layout_with()` keep their signatures and mean "no patterns".
A build without patterns is byte-identical to the build before this ADR.

### 2. A hyphenation point is a boundary kind, not an extra atom

```rust
pub enum BoundaryKind { Normal, Space, Hyphen }
```

`soma-rules::SpanKind::Hyphen` marks a continuation piece of a word: the first
piece is ordinary text, every later piece starts with a hyphenation boundary.
The boundary is `Allowed`, carries `penalty = 100` (cheaper than an overfull
line, dearer than a space), and has zero glue.

Splitting happens in the layout, before the rules layer, because the layout
owns the pattern set. A word is skipped when it is shorter than six letters,
contains an uppercase letter (acronyms), or touches one of `/ : . @ _ -`
(URLs, paths, emails).

### 3. The hyphen is charged to the line

The breaker adds the hyphen glyph's advance to a line's natural width when the
break is at a `Hyphen` boundary. Justification therefore stops at the measure,
not one hyphen past it. `Line.hyphenated` records the fact; the canonical form
writes `"hyphenated":true` only on such lines, so every other line keeps the
bytes it had before.

All three renderers draw one `-` after the last box of a hyphenated line, in
that atom's style. The PDF uses the same font subset as the text; no new face
is embedded.

### 4. A hyphenation point never stretches

Justification is distributed in proportion to each boundary's stretch. An
internal hyphenation point has none, so it gets none: a line made only of word
pieces is left short instead of opening gaps between syllables. Before this,
a line whose boundaries were all zero-stretch fell back to an even spread of
the adjustment; the corpus is unchanged by the fix because such lines did not
occur in prose before hyphenation existed.

## Consequences

- A document without a pattern file is unchanged: no new boundary kinds occur,
  no line is hyphenated, and canonical bytes for existing inputs do not move.
- `Line` gained one field with a default of `false`. ADR-0007 permits exactly
  this: a default-zero extension that does not change existing behaviour.
- `soma-hyphen` is a new crate with no dependencies and no `std`; Gate A lists
  it under `soma-layout`'s allowed dependencies.
- Patterns are not validated for language correctness. A wrong pattern set
  produces wrong breaks, deterministically.
