# ADR-0116: Diagram labels use the text face in the PDF

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0102 (diagram compiler), ADR-0113 (diagram fit),
  ADR-0060 (ruby face rescue)
- Touches: `soma-math` (`MathItem.face`), `soma-layout` (`DiagramText`),
  `soma-render-pdf` (`collect_math`, `paint_math`), tests

## Context

Diagram labels are stored as `MathItem`s in the diagram's `MathBox`. The SVG
proof draws a math text item with the surrounding block's family (the diagram
style), and the layout measured it with that same text face. The PDF backend,
however, sent every math item through `math_face`, which prefers the math face
(NewCM) for Latin characters.

The PDF therefore drew diagram labels in a serif math face even though the
template asked for sans: labels looked foreign next to the body, and their
advances differed from the measured text face, so a label could cross its node
border (`LAN/DAW/MIDI` in the TACTUS architecture diagram). The Studio proof
(SVG) and the exported PDF disagreed on the same document.

## Decision

1. `MathItem` carries a `face: MathFace` (`Math` by default, or `Text`) and a
   `text_face()` builder. `DiagramText::line_of` marks its items `Text`.
2. The PDF's `collect_math` and `paint_math` choose the face per item:
   `Math` keeps the existing math-face resolution; `Text` uses the text role,
   then the rescue face, then the math face for a glyph the text face lacks.
   This is the same order ruby annotations already use; the helper is renamed
   `text_rescue_role`.
3. The hint is not serialized. It is a renderer instruction that is constant for
   a given item, and the canonical layout (and therefore the native/wasm hash)
   must not move for a rendering-only distinction.

## Consequences

- Diagram labels in the PDF use the template's text family, match the SVG proof,
  and advance by the widths the layout measured. Node text no longer overflows
  its border because of a face mismatch.
- Formulas are unchanged: `Math` is the default and keeps the math face,
  including the `\text{...}` fallback behavior.
- `MathItem` gains a public field; all in-tree constructors set `Math` except
  diagram labels. No schema, canonical byte, or hash changes.

## Alternatives considered

- **Add `BoxKind::Diagram` and branch in the renderer.** Rejected: `BoxKind` is
  part of the canonical page result, so the schema and every document hash would
  move for a rendering detail.
- **Infer "label" from the block's node kind.** Rejected: diagrams share
  `BoxKind::Math` with formulas by design; the node does not identify the
  producer after layout.
- **Draw all math text with the text face.** Rejected: a formula's Latin symbols
  must come from the math face to keep their design (italic variables, math
  spacing); only labels opt out.
