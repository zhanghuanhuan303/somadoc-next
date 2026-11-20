# ADR-0047: figures carry image XObjects, and every page is tagged

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0010 (borrow-first layer boundaries), ADR-0034 (PDF backend),
  ADR-0044 (links and decorations), `docs/debt.md` D82
- Touches: `soma-layout` (`TextSource`, `TextIndex`), `soma-render-pdf`,
  `soma-cli`. **No schema change**: `TextIndex` is not part of
  `soma.pages/9`, so the canonical result is unchanged.

## Context

Two gaps were left in D82 after links and underlines landed:

1. **Figures were a grey box.** DocIR has carried `Image.source.src` since A1
   and the layout used the intrinsic pixel size to pick a height, but the
   renderer had no bytes: the renderer never opens files, and the layout is
   `no_std` with no file system.
2. **Nothing was tagged.** `pdfinfo` reported `Tagged: no`, so a screen reader
   saw a page of unlabelled glyphs: no reading order, no headings, no figure
   descriptions.

## Decision

### 1. The source string crosses the layers; the bytes do not

`TextIndex` records `node → (src, alt)` for every figure while the layout
walks the document, and `TextSource` gains three default methods:

```rust
fn block_image(&self, _block: NodeId) -> Option<&str> { None }
fn block_alt(&self, _block: NodeId) -> Option<&str> { None }
fn block_heading_level(&self, _block: NodeId) -> Option<u8> { None }
```

The renderer hands `src` back to the caller through a new seam:

```rust
pub trait ImageSource { fn image(&self, src: &str) -> Option<&[u8]>; }
```

`NoImages` is the null implementation, which is what a renderer without a file
system passes. The CLI implements it by pre-loading every source the document
mentions, resolved relative to the input file.

Two properties fall out of this shape:

* **`TextIndex` is not serialised** (`canonical.rs` says so explicitly), so
  nothing about `soma.pages/9` moves. The page result and its hash are exactly
  what they were before images worked.
* **The renderer stays file-agnostic.** The same `PagedLayout` can be rendered
  from a different asset root, or from memory in a test, which is why the image
  tests need no fixture directory.

### 2. A source never reaches the network

`FileImages` (CLI) skips any `src` containing `://` or starting with `data:`,
and only reads a path joined onto the input document's directory. A document
therefore cannot make the renderer fetch anything, which matches the rule the
HTML and SVG renderers already follow (`docs/capabilities.md`). An asset that
does not load, or a format the backend cannot read, leaves the placeholder box:
one broken figure must not fail the page.

### 3. PNG is decoded, JPEG is passed through

`src/image.rs` produces what an image XObject needs and nothing more.

* **PNG** — 8 bits per sample, no interlacing, colour types 0/2/3/4/6
  (grey, RGB, palette, grey+alpha, RGBA). `src/inflate.rs` decodes the IDAT
  stream (all three deflate block types), scanline filters 0–4 are undone, a
  palette is expanded, and an alpha channel is composited over white with
  integer arithmetic. The samples are then deflated again by the writer with
  `/FlateDecode`.
* **JPEG** — only the frame header is parsed (SOF gives width, height and
  component count); the bytes travel verbatim with `/Filter /DCTDecode`, so no
  entropy decoder is needed and the image is never re-encoded. 1, 3 and 4
  components map to `/DeviceGray`, `/DeviceRGB`, `/DeviceCMYK`.

The inflate decoder is tested in both directions: it decompresses streams our
own deflate encoder produces (fixed and dynamic blocks), plus a hand-built
stored block, and it rejects a corrupted Adler-32.

### 4. One XObject per distinct source, addressed by resource name

`collect_images` walks the pages in reading order and reserves one object per
distinct `src`, named `/Im1`, `/Im2`, … That is deterministic, deduplicates a
figure repeated on several pages, and costs nothing when no figure resolves.
A page's `/Resources` gains `/XObject << /ImN obj 0 R >>` only for the images
it actually draws, and the content stream draws them with
`q W 0 0 H x y cm /ImN Do Q` — the box's size in points, anchored at its
bottom-left corner.

### 5. The structure tree is built while painting

`paint_page` returns the marked-content elements in reading order, and
`render` turns each into a `/StructElem`:

* `/S /P` for paragraphs and table cells, `/S /H1`…`/S /H6` from the heading
  level, `/S /Figure` for figures, `/S /Formula` for display math,
  `/S /Code` for code blocks, `/S /Note` for footnotes;
* a `Figure` also gets `/Alt` from the document's alt text, encoded as UTF-16BE
  when it is not ASCII;
* each element carries `/Pg` (its page) and `/K` (its MCID);
* pages with elements carry `/StructParents n`, and a `/ParentTree` maps `n`
  to that page's elements in MCID order.

Content is wrapped in `/<Tag> << /MCID n >> BDC … EMC`. Running heads, folios
and the footnote area are painted but not tagged: repeating the header in the
reading order on every page would be worse than leaving it out.

A table cell is tagged as a paragraph. `/TD` outside a `/Table` would be an
invalid tree, and the box tree has cells as placed blocks, not rows of a table
structure — inventing that hierarchy is a different job (D51).

## Consequences

- `pdfinfo` reports `Tagged: yes`. The three-page image sample has 19 structure
  elements, one per MCID, and three `/StructParents` entries.
- Tagging costs bytes: `examples/book` went 168,155 → 182,581 B (+8.6%),
  `anthology` 457,559 → 507,290 B (+10.9%). The trade is a page that a screen
  reader can follow.
- `soma-render-pdf::render` takes a sixth argument. Callers pass either a real
  source or `NoImages`; the renderer's output for a document without figures
  changes only by the structure tree.
- HTML and SVG still draw the placeholder rectangle for a figure. They have no
  asset seam yet, and emitting a `data:` URI would move the file reading into
  the renderer. Their capability table says so.
- D82 closes: images, tagged PDF, links and underlines all work. What remains
  is narrow and recorded separately — ICC/CMYK profiles, image scaling modes,
  inline images, and `> 8`-bit PNG.

## Alternatives

- **Bytes in DocIR.** Rejected: DocIR is the semantic layer, and a document
  that embeds its assets cannot be diffed, validated or re-styled. A source
  string plus a caller-supplied store is the same shape as fonts.
- **A `Document`-lifetime reference on the page result.** Rejected in ADR-0010
  and still true: `LayoutResult` must outlive the document so it can be sent
  across threads and to WASM.
- **Decoding JPEG in order to re-encode it.** Pointless and lossy; a reader
  decodes DCT streams natively.
- **A full `/Table` structure with rows and cells.** Correct, and much larger:
  the layout would have to expose row membership. Recorded in D51 instead.
