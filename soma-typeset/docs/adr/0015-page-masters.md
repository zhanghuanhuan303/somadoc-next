# ADR-0015: Page masters (running header, footer, folio)

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0013 (line-level vertical layout), `docs/proposal-effect-max.md` E4
- Supersedes: nothing. Extends the template `page:` block and the page result.

## Context

Pagination produced content boxes and nothing else: no running title, no page
number, no footer. A laid-out document did not look like a page of a book, and
the renderer had no place to put page furniture.

## Decision

Page furniture is **page geometry**, produced by layout, not by the renderer.

Template:

```yaml
page:
  size: A5
  margin: 18mm
  header: 6mm              # band inside the top margin
  footer: 8mm              # band inside the bottom margin
  header-text: running-title   # or a literal string
  folio: "第 {page} 页 / 共 {pages} 页"
```

- Bands live **inside the margins**, not inside the content box, so enabling them
  cannot change pagination. `resolve` rejects a band taller than its margin.
- `running-title` is the plain text of the last heading at or before the page's
  first flow item. Headings are recorded while the flow is built.
- The folio substitutes `{page}` (1-based) and `{pages}` (total). Total page
  count is known after pagination; substituting it cannot change page breaks
  because the band is outside the content box.
- Layout emits ordinary `PlacedBlock`s for header and footer, with synthetic
  `NodeId`s allocated above the document's maximum. Text resolution stays behind
  `TextSource`; `PagedLayout` still owns no references.
- Header/footer text is centered by setting the block's `x`; the block is one
  line at the band height. Long titles overflow rather than wrap (D64).

The renderer draws `Page.header` and `Page.footer` with the same block painter it
uses for content, so there is no second positioning path.

## Consequences

- `soma.pages/3` adds `header` and `footer` arrays; `Page` carries them.
- `soma.template/2` and `soma.styles/2` add the band heights and the two strings.
- A test asserts that turning the master on leaves `Page.blocks` byte-identical.
- A literal header is supported; a per-page or first-page variant is not (D62).
