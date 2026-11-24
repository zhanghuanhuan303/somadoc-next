# ADR-0076: Math geometry follows Typst 0.15 — radical clearance, axis alignment, accents

- Status: Accepted
- Date: 2026-11-24
- Related: ADR-0070 (Math v2), ADR-0075 (design variants), proposal
  `docs/proposal-math-variants.md`, D93, D97, D98
- Touches: `soma.metrics/7`, `tools/metrics-gen`, `soma-core` (three defaulted
  hooks), `soma-metrics` (two sections), `soma-math` (radical, fraction,
  operator, fence, matrix, accent), `soma-layout` (adapters)

## Context

ADR-0075 fixed the gross defects (stroke weight, broken radical link), but a
side-by-side render with **Typst 0.15.1** (`typst-layout/src/math/`) on the same
Noto Sans Math face still showed five differences that the eye notices before it
can name them.

1. **The vinculum touched the radicand.** Typst clears it by
   `radicalVerticalGap` (text) or `radicalDisplayStyleVerticalGap` (display) —
   89‰/190‰ in Noto — and, when the chosen surd is taller than the target,
   distributes the surplus equally above and below
   (`gap = max(gap, (surd.height − thickness − radicand.height + gap) / 2)`).
   Our v6 target used `radicalExtraAscender` instead, which is an *extra
   ascender above the bar*, and left the whole surplus in the surd's leg.
2. **A negative `radicalKernAfterDegree` could move the surd left of the
   cursor.** Typst clamps `sqrt_offset.max(0)` and moves the **degree** right
   by `-min(offset, 0)` instead.
3. **Fractions were shifted from the bar, not from the baseline.** Typst's
   `fraction.rs`: the rule centre is on the axis; `num_shift`/`den_shift` are
   distances from the formula baseline; the minimum gaps are measured from the
   rule's edges. Ours added the axis to both shifts and measured gaps from the
   bar, so the numerator sat too high and the denominator too high by different
   amounts.
4. **Big operators and matrices were baseline-aligned.** Typst centres a Large
   glyph on the mathematics axis (`center_on_axis`) and sets a table's baseline
   to `height / 2 + axis` (`table.rs`). Our operators kept their font baseline
   (0.04 em low for Noto `∑`) and matrices sat on their first row.
5. **Accents were drawn with spacing marks.** Noto Sans Math has the combining
   accents (U+0302/0303/0304/20D7) but not the spacing ones (U+02C6/02DC/00AF),
   so our `\hat`, `\bar`, `\tilde` rendered `.notdef` boxes. Typst aligns an
   accent by the **top accent attachment** of both the base and the accent
   (`MathGlyphInfo/MathTopAccentAttachment`), and accounts for an accent whose
   ink is entirely above its own baseline.

## Decision

### 1. `soma.metrics/7`

Version 7 appends to version 6:

```text
six more MathConstants (after AccentBaseHeight, in `soma_core::MathParam`
order):
  RadicalGap          = radicalVerticalGap
  RadicalDisplayGap   = radicalDisplayStyleVerticalGap
  NumDisplayGap       = fractionNumDisplayStyleGapMin
  DenomDisplayGap     = fractionDenomDisplayStyleGapMin
  SubTopMax           = subscriptTopMax
  SpaceAfterScript    = spaceAfterScript
   (per-mille of one em)

per face:
  u32 math_top_accent_count
  per glyph: varint(codepoint delta) | i16 top accent attachment
```

`soma-core::Metrics` gains three defaulted hooks: `math_top_accent`,
`math_has_glyph` (the math face alone, not the text∪math union) and
`math_glyph_bottom` (signed; negative when the whole glyph is above its
baseline). A host without them keeps the old bytes and the old accent fallback.

### 2. Radical

```text
gap     = display ? radical_display_gap : radical_gap
needed  = radicand height + rule + gap
surd    = first variant with height >= needed, else the last
gap    := max(gap, (surd height - rule - radicand height + gap) / 2)
bar top = baseline - radicand ascent - gap - rule
surd ink top = bar top
offset  = kern_before + degree width + kern_after
surd x  = start + max(offset, 0)
degree x= start + kern_before - min(offset, 0)
```

`radicalExtraAscender` is still read into `MathStyle`, but it is empty space
above the bar, not the bar-to-radicand gap; the layout does not reserve it.

### 3. Axis alignment

- A single-glyph operator (`∑`, `∏`, `∫`, `∮`) is drawn so its ink centre is
  on the axis; its limits and side scripts are shifted by the same amount.
  A word operator (`\lim`, `\sup`, `\inf`) keeps its baseline.
- A stretched fence (chosen variant index > 0) is centred on the axis. A base
  fence keeps its baseline (the design is already axis-centred).
- A matrix/`aligned`/`cases` block shifts every row baseline by
  `(ascent - depth) / 2 - axis`, so the block's ink centre is on the axis.

### 4. Fractions

The rule is centred on the axis. With `axis`, `t = rule thickness`:

```text
num_baseline = -( max(num_shift,
                      axis + t/2 + num_descent + num_gap_min) )
den_baseline = +( max(den_shift,
                     -axis + t/2 + den_ascent  + den_gap_min) )
```

This is Typst's `layout_fraction` expressed in our baseline-relative units.
In display style `num_gap_min`/`den_gap_min` are
`fractionNumDisplayStyleGapMin`/`fractionDenomDisplayStyleGapMin` (150‰ in
Noto, against 71‰ in text style).

### 5. Accents

- Prefer the combining form of a mark when the **math face** has it:
  U+02C6→U+0302, U+02DC→U+0303, U+00AF→U+0304, U+20D7 stays.
- Fall back to the spacing mark, then to a drawn `→`, only when neither face
  has the mark.
- Horizontal: `x = base_x + base_top_accent − accent_top_accent`; defaults are
  the base's width/2 and the accent's advance/2 (Typst's `accent.rs`).
- Vertical: the accent's **signed ink bottom** sits `60‰` above the base's ink
  top, so a combining mark (whole ink above its baseline) is not floated by its
  own ink height.

## Consequences

- Radical bars have the font's designed clearance; a too-tall variant splits
  the surplus above and below instead of leaving one long leg.
- Fractions sit symmetrically around the axis; matrices and operators are
  axis-aligned like TeX/Typst.
- The three spacing accents no longer render as `.notdef` on Noto; `\vec` uses
  the combining arrow.
- `metricsHash` and every page containing the affected constructs change bytes.
  The page schema stays `soma.pages/11`: no new canonical fields.
- The corpus line breaks/pages are unchanged (`ALL,48,599,1213,…`); the
  `math-page` jitter metric moves from 465 to 473 because the math boxes grew
  by the new gaps.
- Still open: `MathKernInfo`, flattened/wide accent variants (horizontal
  variants), glyph assembly (D97, D98). The overbrace still falls back to the
  rule assembly for fonts without a horizontal brace construction; Noto's
  U+23DE has neither a horizontal construction nor assembly.

## Alternatives

- **Keep the bar on the ink top.** Simpler, but visibly tighter than TeX/Typst
  at display sizes; rejected.
- **Use the spacing accents.** They do not exist in Noto Sans Math; the boxes
  are the bug that started this investigation; rejected.
- **Centre only when a variant is stretched.** This is what Typst does for
  fences, but operators are centred unconditionally; we follow that split.
- **Scale the accent to the base width.** Without horizontal variants the
  stroke weight changes; we leave the accent at its design width and only
  stretch spacing marks when the renderer supports it.
