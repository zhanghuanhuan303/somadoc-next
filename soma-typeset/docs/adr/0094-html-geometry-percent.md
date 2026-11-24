# ADR-0094: HTML geometry is page-relative; inline sizes are block-relative

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0044 (decorations/links), ADR-0048 (inline images),
  ADR-0093 (Web Studio), ADR-0059 (hyphenation), ADR-0060 (ruby)
- Touches: `crates/soma-render-html`, its tests; no canonical layout, PDF or
  SVG change

## Context

The HTML renderer wrote absolute positions as `em` on the same element that
carried the block's `font-size`. CSS resolves `em` lengths against the
element's own font size, so a heading at `1.439em` also multiplied its `top`
and `left` by 1.439. Inline atom spans repeated the block's absolute
`font-size` inside the block, squaring the size (`1.439 × 1.439`). The PDF was
correct; the HTML proof showed overlapping cover lines and compressed
headings. This was hidden while the HTML backend was only a geometry proof,
but Web Studio uses it as the proof surface, so it must be faithful.

## Decision

- **Block and page geometry is expressed as a percentage of the page.** A
  position `left: 12.0951%` is independent of the block's font size and
  scales with the page width. The `.page` itself keeps its `em` width/height;
  its font size is the document em.
- **Atom geometry is a percentage of its block.** An atom's `left/top` is
  relative to the positioned block, so percentages of the block's width/height
  are exact and independent of the atom's font.
- **Math spans and rules use the block frame.** Their positions are
  percentages of the block; their internal coordinates remain in math units.
  TikZ SVGs use percentage boxes and keep their `viewBox` in picture units.
- **Inline font sizes are ratios of the block's font.** An atom that differs
  from the block emits `font-size: <atom/block>em`, not the absolute size.
  Line-height is emitted as a ratio of the atom's own font. The block keeps
  the absolute size.
- Footnote numbers and page-level rules use the page frame.

The renderer gains `PageFragment` (`style` + page body) and
`render_fragments(_with)`; joining the fragments with `PAGE_STYLE` produces
byte-identical HTML to `render_with`. The Studio mounts one fragment per
Shadow DOM.

## Consequences

- HTML output positions change from `em` to percentages. The canonical layout,
  PDF bytes, SVG output and corpus baseline do not change.
- HTML tests assert percentages and the fragment/document equality instead of
  exact `em` strings.
- The Studio proof matches the layout's geometry for headings, cover lines,
  inline math and code blocks; font ink remains D101.
- No API or schema change; `render`/`render_with`/`render_page_with` keep their
  signatures.

## Alternatives

- **Use `rem`.** In a Shadow DOM, `rem` refers to the document root, not the
  page host; it would couple the paper to the shell's font size.
- **Use `calc(var(--soma-em) * n)`.** Works, but makes every position a long
  expression and requires a variable at every mount point; percentages are
  simpler and native to CSS.
- **Wrapper elements for each font size.** The atom would still resolve `em`
  against its own font size; the wrapper only moves the problem.
