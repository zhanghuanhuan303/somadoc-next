# ADR-0075: Math variants — design glyphs for operators, radicals and fences

- Status: Accepted
- Date: 2026-11-24
- Related: ADR-0026 (display math), ADR-0027 (inline math), ADR-0030 (math
  completion), ADR-0053 (math baseline), ADR-0070 (Math v2), proposal
  `docs/proposal-math-variants.md`, D93
- Touches: `tools/metrics-gen`, `soma.metrics/6`, `soma-core` (two defaulted
  metrics hooks), `soma-metrics` (variant section), `soma-math` (operator,
  radical and fence geometry, `MathItem`), `soma-layout` (metrics adapters,
  `soma.pages/11`), `soma-render-pdf` (MATH parser, glyph selection, subset),
  `soma-render-svg`/`soma-render-html` (fallback size only), sample scripts

## Context

`∑` came out heavier than the text around it and `√` looked broken. The cause
was one decision applied in three places: the math layout enlarged a **text
size glyph** instead of asking the font for its **designed display glyph**.

- `big()` set the display size to `displayOperatorMinHeight` and scaled the base
  glyph. Noto Sans Math asks for 2.3 em while its `∑` variants stop at 1.34 em,
  so the scaled base glyph was both too wide and much too bold.
- `radical()` did the same (`×9/8` per step) and started the vinculum at the
  surd's **advance**. The glyph's ink ends before its advance (156/1000 em for
  DejaVu, 712/1000 em for its 1.46 em variant), so the bar and the hook never
  met.
- Two independent coordinate bugs made the degree float: the raise used an em
  length where the spec defines a percentage of the radical height, and the
  surd x added the cursor a second time after the degree had already advanced
  it.

The fonts' `MATH` tables already carry the answer: `MathVariants`. All variants
in the three installed math faces are **unencoded glyphs** (Noto: 119 unencoded,
13 encoded), so the fix cannot be a codepoint lookup; it has to be a glyph
selection.

## Decision

### 1. `soma.metrics/6` records vertical variants per reachable base codepoint

```text
u16 math_variant_base_count
per base: varint(cp delta) | u16 count
  per variant: i32 width | i16 top | i16 bottom | i16 right_gap
```

`width` (horizontal advance), `top`/`bottom` (ink) and `right_gap` (`width − ink
right`, clamped at 0) are in 1/4096 em. `metrics-gen` reads `MathVariants → vertGlyphCoverage`
(format 1 or 2) and writes only bases reachable through the cmap. Text faces
write a single zero count. Versions 1–5 still parse and report no variants.

### 2. The metrics seam stays font-agnostic

`soma-core::Metrics` gains two defaulted hooks, `math_variant_count` and
`math_variant`, returning `MathVariant { advance, above, below, right_gap }`.
The default is "no variants", so a host without MATH data keeps the old bytes.
No glyph ids cross the seam; a variant is addressed by **ordinal**, which is
font-independent because the spec orders variants by increasing size.

### 3. `soma-math` selects, it does not scale

- A display operator uses the first variant whose ink height reaches
  `display_operator_min`, else the last; the glyph is drawn at the design size.
- A radical uses the first variant covering the radicand's ink plus the rule and
  extra ascender, else the last. The vinculum starts at `advance − right_gap −
  rule`; the degree's bottom sits at `radicalDegreeBottomRaisePercent` of the
  radical height above the radical's bottom, and `radical_kern_before/after`
  become signed (the fonts supply −450…−526‰).
- A fence picks a variant per side; each side's ink centre meets the body's ink
  centre.
- Without variants, the old scaling path runs unchanged (except the radical
  connection and degree placement, which are bug fixes).

`MathItem` carries `design` (the size for the selected glyph) and `glyph`
(`Text` or `Vertical(i)`); `size` remains the size for drawing `text`.
Renderers that cannot address an unencoded glyph (SVG/HTML) draw `text` at
`size`, which is calibrated to the variant's ink height. Canonical math items
gain `design` and `glyph` (`0` = the text glyph, `N > 0` = vertical variant
`N - 1`); the page schema becomes `soma.pages/11`.

### 4. The PDF backend resolves the ordinal in the actual font

`PdfFont` parses `MATH → MathVariants` on load and maps `base gid → [variant
gid]`. `paint_math` draws `Vertical(i)` by glyph id at `design` size, and
`collect_math` includes those ids in the subset. `Identity-H` and preserved gids
make this a lookup change, not an encoding change.

## Consequences

- Display `∑`/`∏`/`∫`, radical signs and fences use the font's designed sizes,
  so their stroke weight matches the text at every size.
- The radical bar meets the hook, and the degree sits in the nook instead of
  floating above the formula.
- `metricsHash` changes with the regenerated table; every page result containing
  math changes bytes (schema 11). Non-math pages are unchanged.
- GlyphAssembly is not used: all three surveyed math fonts report zero vertical
  parts. The largest variant is the fallback, as in TeX when no larger design
  exists (D97). Horizontal variants and `MathKernInfo` remain open (D97/D98).
- `MathItem` grew two fields; every constructor and the canonical writer were
  updated mechanically.
- Variants are ordinal-addressed, so the metrics table and the PDF's
  `--font-math` must come from the same face. A mismatch still resolves but can
  put the limits at the wrong distance, because the ink the layout used is the
  table's, not the font's. Sample scripts generate the table from the font
  they render with.
