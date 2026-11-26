# ADR-0049: SVG coordinates are layout units, and the root viewport is pixels

- Status: Accepted
- Date: 2026-11-20
- Closes: D87
- Related: ADR-0048 (inline images), ADR-0029 (CLI), Gate R
- Touches: `soma-render-svg` only. No layout change; the golden SVG hash moves
  because the representation changes, not the geometry.

## Context

The SVG renderer wrote every coordinate as an `em` length:

```xml
<svg … width="39.954em" height="56.692em" viewBox="0 0 163655 232214">
  <text x="4.319em" y="6.116em" font-size="1.428em">目录</text>
```

Two things are wrong with that, and they compound.

1. **The units do not match the `viewBox`.** The `viewBox` is in layout units
   (1/4096 em), but `x="4.319em"` is an em length: it resolves against the
   *element's* font size. So the text's position depended on the font size,
   which is `1.428em` of the parent — a position the layout never computed. The
   output looked plausible in a browser only because both the coordinate and
   the font size were wrong in the same direction, and it fell apart the moment
   a page element was rasterised on its own.
2. **The root viewport had no absolute size.** `width="39.954em"` with no
   `font-size` context resolves to whatever the consumer defaults to (12 px in
   `rsvg-convert`, 16 px in a browser), so the same file was a different size
   in different tools.

D87 recorded the second symptom — a blank page from `rsvg-convert` — as a
cosmetic wart. It was the first one.

## Decision

### 1. Coordinates are unitless layout units

`x`, `y`, `width`, `height` and `font-size` are written as bare numbers in the
same units as the `viewBox`. The renderer's helper is now
`pub fn units(Width) -> String`, and it is public so the tests format a
coordinate exactly the way the renderer does instead of duplicating the rule.

`font-size` as a unitless number is a presentation attribute in user units,
which every SVG engine accepts; that is what makes the text scale with the
`viewBox` rather than with an inherited font size.

### 2. The root viewport is pixels at one em = 16 px

```xml
<svg width="639" height="907" viewBox="0 0 163655 232214">
```

`px()` converts layout units to pixels: one em is 16 CSS pixels, so a page is
`units / 256` pixels across. The physical size is arbitrary — SVG scales — but
it is now *stated*, so a rasteriser agrees with a browser and `rsvg-convert`
produces a sane canvas instead of guessing.

### 3. The tests share the format, not a copy of it

Every SVG test had its own `em()` helper that reimplemented the old rule. They
now delegate to `soma_render_svg::units`, so a future format change cannot
leave the tests asserting a shape the renderer no longer emits.

## Consequences

- A page extracted from the output rasterises correctly: the three-page figure
  sample renders its heading, body text, image, caption and folio at the right
  positions in `rsvg-convert`.
- The golden SVG hash changes (`2215190967058887527` → `9092984200964852902`)
  even though every coordinate keeps its value: the string is different, the
  geometry is not. Gate R's comment records why.
- Nothing else changes. The HTML renderer keeps `em` in its CSS, where the
  unit is resolved by the browser against the page's own font size and means
  what it says.

## Alternatives

- **Keep `em` and add a root `font-size`.** Fixes the viewport but not the
  coordinates: `x` would still be an em length whose basis is the text's own
  font size, not the layout's.
- **Emit `pt` everywhere.** Correct, but needs the body point size, which the
  SVG renderer does not receive; it would also make the `viewBox` and the
  coordinates disagree unless the `viewBox` were rewritten too.
- **Emit one `<svg>` per page in a wrapper document.** Would make the file
  valid as a whole, but a per-page element is what previews and Gate R want,
  and the fix above makes each element stand on its own anyway.
