# ADR-0066: Word styles and embedded images

- Status: Accepted
- Date: 2026-11-21
- Closes: the styles half of D24 (images: import, not layout fidelity)
- Related: ADR-0063 (Word reader), N1 (no layout fidelity),
  `docs/debt.md` D24
- Touches: `soma-docx` (`styles.rs`, `media.rs`, `parse_docx_with_media`),
  `soma-cli` (embedded media reach the renderers)

## Context

The first Word reader mapped `w:b`/`w:i` written directly on a run and looked
for literal `Heading1` style ids. Real documents put emphasis and heading
identity in `styles.xml`, and images live in `word/media/` with the paragraph
pointing at a relationship id. Both were dropped, so a normal Word manuscript
lost its structure and every illustration.

## Decision

### 1. Style inheritance, resolved once

`styles.xml` is parsed into paragraph and character styles. A style's
properties are the overlay of its `w:basedOn` ancestors, root first; a repeated
id stops the walk, so a malformed cycle cannot hang.

Applied precedence, lowest first:

1. character properties carried by the paragraph style;
2. the run's `w:rStyle` character style;
3. the run's own `w:rPr`.

`w:b` / `w:i` / `w:u` without `w:val` mean on; `0`, `false`, `off`, `none`
mean off. `w:vertAlign` maps to superscript/subscript, and `baseline` clears an
inherited shift.

Heading level is decided by the style **name** (`heading 1` … `heading 9`,
`Heading1` …, `Title`), not by the id, because Word localises and renames ids.
Without `styles.xml` the reader falls back to the old literal-id rule, so
documents that were readable stay readable. DocIR headings cap at level 6.

### 2. Images become figures, bytes travel with the document

A `w:drawing` (or VML `v:imagedata`) resolves `r:embed` through
`word/_rels/document.xml.rels` to a zip entry. The paragraph is split around
the image: text before, `Block::Figure`, text after. Size comes from
`wp:extent` (EMU / 9525 = pixels at 96 dpi); without an extent the layout falls
back to its own aspect heuristic.

The archive's media are returned beside the document:

```rust
pub fn parse_docx_with_media(bytes: &[u8]) -> Result<(Document, Vec<MediaFile>), DocxError>
```

`ImageRef.src` is the name inside the archive (`media/image1.png`), never a
filesystem path. The CLI hands the list to `FileImages`, which already resolves
`src` for the renderers; a file next to the input with the same name wins, so
an author can override an image without editing the archive. `parse_docx`
keeps its signature and discards media.

## Consequences

- PDF/HTML/SVG render the image exactly as they render a Markdown or LaTeX
  figure; no renderer change was needed.
- Floating position, wrap, crop, rotation, alt text, linked (external) images,
  theme colours, `docDefaults` and `numbering.xml` are still not read. A float
  becomes a normal figure in flow order.
- An image format the PDF backend cannot decode still gets the placeholder box,
  which is the existing figure contract.
