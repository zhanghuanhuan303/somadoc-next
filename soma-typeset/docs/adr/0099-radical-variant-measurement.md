# ADR-0099: Radical variants are measured from the right outline, and text renderers place their fallback

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0073 (CFF ink), ADR-0075 (design variants), ADR-0097 (display
  math frame), ADR-0098 (short display skip)
- Touches: `tools/metrics-gen`, the committed metrics tables,
  `soma-math::MathItem`, `soma-render-html`, `soma-render-svg`,
  `soma-studio-ui`, the metrics-gen test

## Context

Two symptoms had one root and one renderer gap:

1. **PDF: the vinculum did not meet the surd.** The larger radical variants
   carry an optional Type 2 *width* as the first operand of their first
   stack-clearing operator. The bbox-only CFF interpreter pushed that width on
   the operand stack instead of dropping it, so every later coordinate was
   shifted: the measured ink was about 0.36 em too high (and the total height
   was still right, which hid the error). The layout then placed the vinculum
   above the drawn surd.
2. **Studio preview: √ drifted.** HTML and SVG cannot address an unencoded
   design variant, so they draw the fallback text glyph at `MathItem.size`.
   They used the variant's `y` and the variant's advance: the fallback glyph is
   wider than a tall variant and its ink top is not at the variant's ink top,
   so the glyph and the separately drawn rule separated. The proof also used
   the body face for `.math`, not the configured math face.

## Decision

- **Drop the optional Type 2 width.** `cff::run` consumes one leading width
  operand before the first stem/move/endchar operator; the ink box then measures
  the real outline. The committed `noto-cjk.smt` and `metrics-lxgw.smt` are
  regenerated with the fixed generator. A test parses the bundled NewCM Math
  face and locks the radical variant rows.
- **Text renderers use the fallback's geometry.** `MathItem` gains two
  renderer-only fields (not in canonical JSON):
  - `text_y`: the baseline at which the fallback glyph's ink top meets the
    design variant's ink top;
  - `text_scale_x`: the horizontal compression (permille) that makes the
    fallback glyph's advance match the variant's. Tall radicals are otherwise
    several times wider than the variant and overlap the radicand.
  HTML and SVG draw the text glyph at `size`, `text_y` and `text_scale_x`; PDF
  still draws the real variant glyph at `y` and `design`.
- **Studio uses the math face in the proof.** The shadow CSS maps `.math` to
  `--soma-math`, and the math font URL is registered as `opentype`.

## Consequences

- PDF radicals connect: the vinculum starts at the variant's ink right and the
  surd's ink top meets it. Nested radicals and cube roots are fixed.
- The Studio proof draws a compressed base radical instead of the variant; it
  is not pixel-identical to the PDF, but its ink top and advance now match the
  variant, so it no longer drifts or overlaps the radicand.
- The table bytes change only in the math variant rows; body faces and every
  line/page count in the corpus are unchanged. `metricsHash` and therefore the
  canonical page hash of a math document change; no schema version changes.
- `MathItem.text_y`/`text_scale_x` are not serialized, so the canonical JSON
  and its hash do not contain renderer-only fallback geometry.

## Alternatives

- **Ship variant glyph outlines to HTML/SVG as paths.** Exact, but would add a
  second outline pipeline and a large payload; the base glyph is adequate for a
  proof.
- **Use the base glyph uniformly scaled.** It overlaps the body for tall
  radicals; the scale factor is the reason for the second field.
- **Leave the CFF width bug and patch only the radical table values.** Would
  hide the bug for this font but leave every other width-bearing CFF glyph
  measured wrong.
