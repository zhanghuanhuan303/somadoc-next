# ADR-0044: underlines and links cross the layers as atom attributes

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0004 (frontends), ADR-0011 (inline span compiler), ADR-0034
  (PDF backend), `docs/debt.md` D82
- Touches: `soma-core` (`Atom`), `soma-rules`, `soma-layout` (`TextSource`),
  `soma-render-pdf`, `soma-render-svg`, `soma-render-html`, `soma-md`.
  No page schema change; `soma.layout` stays at version 6.

## Context

DocIR has carried `Inline::Underline`, `Inline::Strikethrough` and
`Inline::Link` since A1, and the HTML and LaTeX frontends already produced
them. But the span compiler threw the information away: `Atom` had no field for
it, so a renderer could not know that a run of text was underlined or where a
link target was. The PDF backend printed the glyphs and stopped.

Two questions had to be answered before any renderer could draw:

1. **Where does the decoration live?** It could be a `StyleId` (a computed
   style property, like `color`) or an atom attribute (per-atom data).
2. **How does a link become a PDF annotation?** An annotation is a rectangle on
   a page, and the renderer only sees atoms with x/advance, not line boxes.

## Decision

### 1. Decoration and link are atom attributes, not styles

`Atom` gains:

```rust
pub decor: Decorations,      // bit flags: UNDERLINE, STRIKETHROUGH
pub link: Option<Arc<str>>,  // the target, shared across a run
```

The alternative — making underline a property of the computed style, so that
`TextIndex` need not carry it — was rejected because the two are not the same
kind of fact:

* **Style** is chosen by the template (a heading is bold because the template
  says so) and is resolved *per style id*, so two atoms with the same style are
  interchangeable. A link target is not: two adjacent `<a>` elements with the
  same style can point at different URLs.
* **Decoration** could in principle be a style property, but then the template
  could underline text the source did not, and a source would have no way to
  prevent it. The project's rule is that presentation belongs to the template
  and semantics belong to the document (ADR-0009); "this text is underlined"
  is what the author wrote.

`Arc<str>` rather than `String` because a link usually covers several atoms
and the target is identical across them; the page result's text index stores a
`Vec<TextEntry>` and hands out `&str`.

### 2. The breaker ignores both

`decor` and `link` change no width and no break opportunity, so `soma-core`
never reads them: the cost model, the trim rules and the DP are untouched. That
is why `soma.layout/6` did not have to bump — the boxes and coordinates are the
same bytes as before.

### 3. Renderers get the answer from `TextSource`

`soma-layout` keeps the atoms' decorations in its text index and exposes two
new methods on `TextSource`:

```rust
fn atom_decorations(&self, node: NodeId, atom: AtomId) -> Decorations;
fn atom_link(&self, node: NodeId, atom: AtomId) -> Option<&str>;
```

The renderer asks the index what the author asked for; it does not look at
`Inline` nodes itself. This preserves the one-way dependency (ADR-0010): a
renderer consumes `PagedLayout` plus `TextSource`, never DocIR.

### 4. PDF: a filled rule and a `/Link` annotation

* **Underline geometry comes from the font.** `post`'s header carries
  `underlinePosition` and `underlineThickness` in every version, including the
  version-3 faces that omit the glyph names, so `PdfFont` reads them and
  `PdfFontDescriptor` scales them into 1000-unit space. When the table is
  missing or reports nonsense, the fallback is `-upem/10` and `upem/20`, the
  same defaults Typst and TeX use. The renderer never guesses.
* **The rectangle is flipped once, in one function.** Layout baselines count
  down from the page top; a PDF `re f` rectangle anchors at the bottom-left
  corner and grows upwards. `decoration_rect` is the only place that knows
  both conventions, and it takes the thickness in layout units — scaling by the
  atom's `font_size`, not by the milli-point size. Getting this wrong put every
  underline at the bottom of the page during development, so the geometry is
  unit-tested directly.
* **Strikethrough sits at 3/10 em above the baseline**, the usual convention;
  no font table describes it.
* **A link becomes one `/Subtype /Link` annotation per run.** Adjacent atoms
  with the same target merge when their rectangles touch, with a tolerance of
  0.128 pt to absorb the milli-point rounding. Rectangles are
  `[x_left, baseline - ascent, x_right, baseline + descent]` flipped into PDF
  space, so clicking anywhere in the glyph band works.

### 5. HTML and SVG deliberately do not emit `<a href>`

`docs/capabilities.md` promises that the renderers do not create external
references from document content — a link in a source produces text, not a
live URL, and the safety test `links_and_images_do_not_create_external_references`
enforces it. Both renderers therefore emit an underline-styled run and no
anchor. A caller that wants live links can post-process the HTML; the core will
not decide that a document is allowed to make the browser fetch something.

### 6. Markdown gains `~~strikethrough~~`

GFM spells strikethrough `~~`. The frontend's emphasis loop was extended rather
than duplicated, with the delimiters tried longest-first so `**` never loses to
`*`. There is still no Markdown spelling for underline; `<u>` via the HTML
frontend and `\underline` via LaTeX both work.

## Consequences

- `Atom` grows from `{text, kind, class, style, compress_left/right}` to also
  carry `decor: u8` and an `Option<Arc<str>>`. `Atom` is `Clone`, not `Copy`,
  so literals in tests and fixtures gained two fields; the compiler found all
  of them.
- PDF output gains `re f` operators inside the text object (the painter closes
  the text object first) and `/Annots [ … ]` on pages that have links. Pages
  without links and without decorations are byte-identical to before.
- The underline is drawn per atom, so a run that breaks across lines gets one
  rule per line and none in the gap — which is what a reader expects.
- Synthetic styles cannot add a decoration. A template that wants a decorative
  rule under a heading should use the existing `Rule` box, not underline.
- D82 loses "no annotations" and "underlines are not drawn"; images (XObject)
  and tagged PDF remain.

## Alternatives

- **Decoration as a style property.** Rejected above: it lets the template
  contradict the source, and it makes `TextSource` insufficient for links.
- **A separate decoration list in the page result** (like `marks`). The marks
  list exists because a footnote marker *replaces* glyphs, which changes the
  atom stream; a decoration does not. An atom attribute is cheaper and cannot
  drift out of sync with the boxes.
- **Drawing the underline under the whole line** instead of per atom. This is
  what most word processors do, and it is wrong for a run that is only part of
  a line — and it cannot express a link that covers half a line.
