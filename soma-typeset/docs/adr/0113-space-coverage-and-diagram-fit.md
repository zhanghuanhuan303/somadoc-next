# ADR-0113: Measure the space, cover the glyph, fit the diagram

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0111/0112 (code breaks), ADR-0102 (diagram compiler),
  ADR-0077 (metrics coverage), ADR-0037 (outline ink)
- Touches: `soma-metrics` (`Face::has`, `Face::advance_at`), `soma-layout`
  (`StyledMetrics::face_for`, `diagram_item`, `DiagramText::wrap`),
  `soma-render-pdf` (`role_for_text`), tests, docs

## Context

A second review of the TACTUS proposal found three defects that the earlier
line-breaking pass did not cover:

1. `±20 µs` extracted as `±20 �s` and drew a `.notdef` box for `µ`.
2. Inline code such as `sample = round(tick * …)` had spaces in the wrong
   places: `sample=` and `tick*` touched, while the gap appeared after the
   operator. Code blocks had the same measurement error.
3. A wide architecture diagram was scaled to about 60%, leaving 6.5–8.8 pt
   labels.

The causes were independent:

- The metrics table stores a face's space in the face header (`space`), not as
  a character entry. `Face::has` therefore said the face did not cover
  `"sample "`, and `Face::advance_at(' ')` returned `default_advance` (one em)
  instead of the real space (0.25 em for text, 0.6 em for mono). ADR-0112's
  math-face rescue then measured a code atom with the math face, which covers
  the space, while the renderer drew it in mono.
- The PDF writer chose one face per atom from the style and the layout's
  fallback flag. When neither the style face nor the configured rescue face
  had the glyph (Plex has `±` but no `µ`; the Studio's rescue face was Plex
  itself), the atom was drawn with `.notdef` and the text layer carried
  U+FFFD.
- The diagram laid every label out under a 16 em cap. A five-column flowchart
  with 16 em nodes is wider than the measure, so the whole picture was scaled
  down; the labels shrank with it.

## Decision

1. **The space is a character the face covers.** `Face::has(' ')` returns true
   and `Face::advance_at(' ')` returns `Face::space` (an explicitly recorded
   entry still wins). A run that contains a space now measures the width the
   renderer actually advances. This fixes inline code and code blocks.
2. **A text atom may be measured in the math face.** `StyledMetrics::face_for`
   tries the style face, then the rescue family, then the math face. The
   layout and the PDF writer therefore agree on which face draws a symbol that
   only the math face has.
3. **The PDF writer picks a face that covers the whole atom.** `role_for_text`
   tries the style role, the rescue face, the math face and finally the body
   face. If none covers every character, the style role is kept (a visible
   `.notdef` is better than silently dropping the atom). The text layer keeps
   the real codepoint whenever a covering face exists.
4. **The diagram fits its labels.** `diagram_item` lays the diagram out with
   label caps of 16, 12, 10 and 8 em and keeps the candidate with the largest
   page scale, rejecting a candidate below the 60% floor or taller than the
   page. A tie keeps the first (least-wrapped) layout. Diagram labels may also
   wrap after `/` or `-`, which keeps a long platform list from forcing a wide
   node.

## Consequences

- `±20 µs` draws and extracts correctly; the glyph comes from the configured
  math face when the Latin face lacks it.
- Inline code and code blocks use the face's real space width. The corpus
  baseline is unchanged because the corpus has no code atom with an internal
  space that moves a break.
- A wide flowchart wraps its node labels and stays near its design size
  instead of shrinking to the 60% floor. It becomes taller, so a document
  with wide diagrams may break pages differently; the TACTUS proposal keeps
  its 13 pages.
- No schema, template, or parameter changes: the fixes are in measurement,
  face selection and diagram fitting.

## Alternatives considered

- **Record U+0020 as an ordinary entry when generating the table.** Rejected
  as the only fix: committed tables would still need the special case, and
  `Face::space` is already the authoritative value the table carries.
- **Split a PDF text atom into one run per face.** Rejected for now: the atom
  is the natural unit (code is already split by the rules layer), and an
  all-characters check keeps one `Tj` per atom. Per-rune runs would need
  advance bookkeeping inside an atom for a case the atom splitter already
  avoids.
- **Scale diagram labels less than the geometry.** Rejected: labels would
  overflow their nodes. Wrapping is the honest way to narrow the picture.
- **Use a fixed 10 em label cap everywhere.** Rejected: diagrams with short
  labels do not need extra wrapping; the fit search applies it only when it
  buys a larger final scale.
