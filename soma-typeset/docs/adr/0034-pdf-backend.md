# ADR-0034: PDF output with embedded TrueType subsets, bookmarks and page labels

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0010 (renderer boundary), ADR-0033
  (outline and index), `docs/cli.md`, `docs/debt.md`
- Touches: `soma-render-pdf` (new crate), `soma-template` (`StyledDoc::body_pt`,
  `soma.styles/7`), `soma-cli` (`--format pdf`, `--font*`), `scripts/book.sh`,
  `scripts/gate-a.sh`, `scripts/gates.sh`

## Context

The engine produced HTML, SVG and canonical JSON, but no printable file. N2
listed a PDF renderer as a non-goal until there was a demand for print delivery.
That demand is now explicit. The constraints are the project's usual ones:

- the renderer must consume `PagedLayout` + `StyledDoc` + `TextSource`, never
  DocIR, and must not re-break or re-paginate;
- output must be byte-deterministic for a given layout and font files;
- Chinese text needs a real CJK font, not the PDF base-14 fonts;
- the workspace has no third-party dependencies and the renderer should stay
  `no_std + alloc`;
- the bookmark tree must come from the layout's `outline`, not be re-derived
  from headings.

## Decision

### A separate crate at the renderer boundary

`soma-render-pdf` exposes:

```rust
pub fn render(
    layout: &PagedLayout,
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    metadata: &PdfMetadata<'_>,
) -> Result<Vec<u8>, PdfError>
```

Fonts are supplied by the caller (the CLI reads them from disk). The crate is
`no_std + alloc`, forbids `unsafe`, and has no dependencies beyond
`soma-core`/`soma-layout`/`soma-template`.

### TrueType only, sparse subset, identity CIDs

The PDF text object is a Type0 font with `Identity-H`, CIDFontType2,
`CIDToGIDMap /Identity`, a `W` array for the used glyphs, and a `ToUnicode`
CMap. Each used glyph id is also the PDF CID, so layout, shaping and the PDF
agree without a second mapping.

The font program is a sparse subset:

1. Parse `head`, `hhea`, `hmtx`, `maxp`, `loca`, `glyf`, `cmap` (formats 4 and
   12 for Unicode lookup).
2. Collect the glyphs the layout uses, plus composite components, plus
   `.notdef`.
3. Copy only those `glyf` entries; keep every other glyph id as an empty glyph
   and rebuild `loca` in long format.
4. Copy `head` (with `indexToLocFormat = 1`), `hhea`, `hmtx`, `maxp`, and the
   optional hinting tables; rebuild the sfnt directory and checksums, including
   `head.checkSumAdjustment`.

CFF/OTF faces are rejected with a clear error: subsetting CFF is a different
problem, and embedding a full CFF only to produce a font that some readers
mis-handle is worse than saying so.

### Coordinates are integer milli-points

`StyledDoc` now exposes `body_pt()` (the root font size in points; it is also in
the `soma.styles/7` canonical JSON). Layout units (1/4096 em) convert to
milli-points with integer arithmetic:

```text
pt_milli = units * body_pt_num * 1000 / (4096 * body_pt_den)
```

PDF's origin is bottom-left, so every `y` is `page_height - layout_top`. Text is
placed per atom with `Tm`, using the layout's `x` and `line.ascent`; the
renderer never recomputes advances or breaks. Colors are emitted when they
change; rules and fraction bars are filled rectangles; figures are a grey
placeholder frame (image embedding is not part of this ADR). Math items and
inline math are painted from `MathBox`/`InlineMathBox`, with a separate math
face when the caller provides one.

### Navigation comes from the layout

- `layout.outline` becomes a nested PDF outline tree: `/First`, `/Last`,
  `/Prev`, `/Next`, `/Count`, and `/Dest [page /XYZ 0 height null]`. Titles are
  UTF-16BE hex strings, so Chinese bookmarks extract correctly.
- `Page.printed` becomes a `/PageLabels` number tree: roman front matter uses
  `/S /r`, body pages `/S /D`, hidden front matter is skipped.
- The catalog sets `/PageMode /UseOutlines` and `/Lang`; the Info dictionary
  carries title and author.

### Determinism

The file has no timestamps, no compression (so no third-party flate), a fixed
object order (catalog, pages, fonts, page content, outline, labels, info), and
uses only integer arithmetic. Rendering the same layout with the same font files
twice produces identical bytes.

## Consequences

- `soma render --format pdf --font ...` produces an 11-page book (512 KB with
  MiSans; the size is the subset `glyf` plus content streams), with 10
  bookmarks, page labels and text extraction that preserves Chinese.
- The engine can now deliver a printable artifact without a browser or a LaTeX
  installation.
- New debt (D82): no image/XObject embedding, CFF fonts rejected, no tagged
  PDF/accessibility tree, no link annotations, no compression, and underlines
  are not painted.
- `--format pdf` is CLI-only; the renderer still compiles for wasm but the
  browser build does not ship font bytes.

## Alternatives rejected

- **Depend on a PDF crate.** Breaks the zero-dependency, `no_std` rule and
  hides object ordering, which the determinism gate relies on.
- **Embed the full font.** A Noto CJK face is ~20 MB; sparse subsets keep the
  example at half a megabyte.
- **Rename glyphs and rebuild `cmap`.** Renumbering forces a new CID map and a
  new ToUnicode; keeping ids is simpler and keeps layout/PDF in sync.
- **Flatten the outline.** A flat list loses the chapter/section hierarchy that
  readers navigate.
- **Write page labels from a template string.** The layout already decided what
  each page prints (`i`, `ii`, `1`); re-parsing the template would let the two
  drift.
- **Compress streams with a hand-written deflate.** Correctness and
  determinism first; the subset font dominates the file size anyway.
