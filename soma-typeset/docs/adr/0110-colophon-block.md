# ADR-0110: The colophon is a ruled, multi-line block

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0106 (book layout pass), ADR-0105 (line trim), ADR-0107
  (LaTeX metadata)
- Touches: `soma-layout` (`cover_rows`, `CoverStyles`), `soma-template`
  (`ELEMENTS`, `CoverStyles`), `soma-latex` (`\colophon`), `examples/book/song.yaml`,
  `examples/book/full.tex`, tests

## Context

The colophon was one centered line:

```text
soma-typeset 0.1.0 · 全能力排样 · 2026 · Apache-2.0
```

It read as a run-on sentence, not a designed colophon. The mixed line also had
uneven gaps around the middle dots: the CJK/Latin boundary rule redistributes
the spaces around a punctuation atom depending on its neighbours, so the dot
after `0.1.0` could touch the number while the dot after `样` had a normal
space. No single `letter-spacing` value fixes that: the problem is the
structure, not the constant.

## Decision

1. `\colophon{...}` accepts `\\` as a line break. The LaTeX frontend stores the
   trimmed lines joined by `\n` in `cover.fields["colophon"]`.
2. The cover renders the colophon as a block:
   - a `BoxKind::Rule` item, 40% of the measure wide, centred, 1/32 em thick,
     styled by the generated `coverColophonRule` element;
   - one paragraph per line, styled by `coverColophon`. The first line starts
     after the rule's `space-after`; later lines are consecutive paragraphs with
     the same line height, so there is no per-line `space-before`.
3. `song.yaml` sets the rule to 34mm before, 1em after, `#b3b3b3`, and the
   colophon to 8pt/1.5, 60% ink, 0.04em tracking, centred.
4. The sample authors the intended hierarchy explicitly:

   ```latex
   \colophon{soma-typeset 0.1.0\\全能力排样\\2026 · Apache-2.0}
   ```

   so the project/version, the work and the year/licence are separate lines.
   The only remaining separator line is Latin/numeric, where ordinary spaces
   are stable.

## Consequences

- A colophon has a visible top rule and a three-level reading order instead of
  one mixed-script line.
- Single-line colophons still work: a value without `\\` is one paragraph and
  still gets the rule.
- The rule is a layout block, so PDF, SVG and the Studio proof draw the same
  geometry through their existing `BoxKind::Rule` paths.
- The template can restyle the rule (`coverColophonRule`) and the text
  (`coverColophon`) independently without changing the engine.
