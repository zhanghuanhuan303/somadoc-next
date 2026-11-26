# ADR-0048: HTML and SVG inline a figure as a `data:` URI

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0034 (PDF backend), ADR-0047 (image XObjects and tagged PDF),
  `docs/capabilities.md`
- Touches: `soma-layout` (the image seam moves there), `soma-render-html`,
  `soma-render-svg`, `soma-cli`. No schema change; no layout change.

## Context

ADR-0047 gave the PDF backend real figures but left the HTML and SVG renderers
drawing the grey placeholder rectangle, because they had no way to reach the
bytes. Both renderers had a rule — stated in `docs/capabilities.md` and
enforced by `tests/safety.rs` — that the output must not create external
references. That rule is why the placeholder was acceptable: an `<img src>` that
pointed at a file on disk is exactly the external reference the rule forbids.

But "no external reference" and "no image" are not the same requirement. An
image encoded **inside** the document is self-contained: it cannot phone home,
it survives being copied as a single file, and it does not tell the reader
anything about the author's file system.

## Decision

### 1. The image seam moves to `soma-layout`

`ImageSource` and `NoImages` used to live in `soma-render-pdf`. Three renderers
now need the same trait, and none of them should depend on another renderer, so
the trait moved next to `TextSource` in `soma-layout`. `soma-render-pdf`
re-exports both names, so its callers are unchanged.

`soma-layout` also gains one helper, `data_uri(bytes) -> Option<String>`: it
sniffs the PNG/JPEG signature and base64-encodes the bytes. Putting it here
keeps the encoder in one place instead of three, and keeps the renderers
free of encoding code.

### 2. `render` keeps its signature; `render_with` takes the assets

A renderer that suddenly demanded an `ImageSource` would break every existing
caller for no benefit. So:

```rust
pub fn render(layout, styles, text) -> String          // == render_with(…, &NoImages)
pub fn render_with(layout, styles, text, assets) -> String
```

`render_page` gets the same treatment (`render_page_with`). The CLI builds one
`FileImages` per run and passes it to whichever renderer the format selects,
so the asset policy — local paths only, no `://`, no `data:` in the source
string — is applied once.

### 3. A resolved figure becomes an inline URI

* HTML: `<img class="figure" alt="…" style="left/top/width/height" src="data:image/png;base64,…">`
* SVG: `<image … preserveAspectRatio="none" href="data:image/png;base64,…"/>`

The `alt` text comes from `TextSource::block_alt`, escaped. `preserveAspectRatio="none"`
because the box size is the layout's decision: the breaker measured the figure
with the intrinsic aspect ratio, so the image must fill the box exactly, not
re-derive its own.

SVG 2 spells the attribute `href`; SVG 1.1 spellings (`xlink:href`) are not
also emitted. Every current engine reads `href`, and emitting both doubles the
URI in the output for no gain.

### 4. The safety rule is restated, not weakened

The invariant is "no external reference". The test in
`soma-render-html/tests/safety.rs` now scans every `src="` and `href="` in the
output and asserts the value starts with `data:`. Anything else — a path, a
URL, a scheme-less file name — fails. A new test asserts that a source path
like `fig/chart.png` never appears in the output at all.

## Consequences

- HTML and SVG output now carry the image bytes. A 3-page figure sample is
  100,731 B of HTML and 96,838 B of SVG, of which the two images are ~95 KB —
  the base64 expansion (4/3) is the whole difference.
- A document without figures produces byte-identical HTML and SVG to before
  (`render` still passes `NoImages`), so Gate R's golden hash did not move.
- `data_uri` returns `None` for a format it does not recognise; the figure then
  falls back to the placeholder, exactly as in the PDF backend.
- The remaining wart is older than this ADR: the SVG root uses
  `width="39.954em"`, so a page element extracted on its own does not rasterise
  at the right size in `rsvg-convert`. Browsers resolve it. Recorded as D87.

## Alternatives

- **`<img src="fig/chart.png">`.** Rejected: it is an external reference, it
  breaks the moment the HTML is moved, and it leaks the author's directory
  layout into a shared file.
- **A `file://` URI.** Same objection, plus browsers refuse it from `http`
  documents.
- **Ship a sidecar `assets/` directory.** Works, but makes the HTML no longer
  a single file; the whole point of these renderers is a portable preview.
- **Decode and re-encode as PNG in the SVG.** Pointless: a `data:` URI accepts
  JPEG verbatim, and the browser already has a decoder.
