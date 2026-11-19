# ADR-0028: Table of contents and section-start masters

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0013 (line-level pagination), ADR-0021 (named masters),
  `docs/debt.md` D62/D73
- Touches: `soma-template` (`toc:`, `page.start-master`), `soma-layout`
  (TOC generation, page selection), page schema `soma.pages/7`

## Context

Named masters exist and `Section.page_template` switches one, but two things a
book template needs were missing:

1. **A table of contents.** DocIR has `GeneratedKind::Toc` as a placeholder, but
   nothing fills it: page numbers are only known after pagination, so the
   template layer cannot produce the entries.
2. **A chapter-opening master.** Book design usually drops the running head on
   the page that opens a chapter. Masters were selected only by
   `Section.page_template`, which is a *document* decision, not a page-position
   one.

## Decision

### The template asks for a TOC; the layout fills it

```yaml
toc:
  depth: 2        # 1..3, default 2
  title: 目录      # empty value: no heading
```

The template decides *whether* a contents page exists and how it looks; the
document provides the headings. Two style elements exist for it:

- `element: toc` styles one entry;
- `element: tocTitle` styles the heading.

Both inherit from `document`, so a template that says nothing still works.

### Page numbers are a fixed point, not an afterthought

`layout_with` becomes an outer loop:

1. lay out with the entries from the previous pass (empty on pass 1);
2. record which page each heading landed on;
3. if the entries are unchanged, stop; otherwise re-run with the new entries.

Four passes is the cap (`TOC_MAX_PASSES`). If the entries are still moving, the
result is returned with `toc.stable = false` and the numbers printed come from
the previous pass — never silently upgraded.

A heading's page is found from the paginator's own output: every vertical item
knows its flow-item index, so `page_of_item[heading.item]` is exact.

### Row shape

Each row is **one flow item with two boxes**: the title (indented by
`1.5em × (level − 1)`, width reduced by a four-digit number reserve) and the page
number in a narrow box flush with the right edge. One flow item means a row can
never be split across pages.

No dot leaders: leaders need a fill/tab concept the paragraph model does not
have (D73).

The contents end with a forced break, so the document always starts on a new
page.

### Synthetic nodes

Generated rows need `NodeId`s for the text index. They are allocated above the
document's highest id (`Document::node_ids()`), deterministically, per pass.

### Section-start master

```yaml
page:
  start-master: opening   # a master name, or `default`
```

The layout marks two kinds of "chapter opener":

- the first content item after each `Section` marker;
- every level-1 heading.

When a page begins with a marked item, it uses the start master instead of the
flow item's master. The content box is unchanged (masters only change margin
bands), so pagination and page count are unaffected.

An opener that does not begin a page (a `Section` with no page break, a level-1
heading in the middle of a page) does not trigger the start master: the special
page simply does not exist. Level-1 headings are included because most
documents express chapters with headings, not with `Section` markers.

### Schema

Template becomes `soma.template/6` (`toc`, `page.start-master`), resolved styles
`soma.styles/3` (`toc`, `startMaster`), page result `soma.pages/7`
(`"toc": null | {passes, stable, entries}`).

## Consequences

- A template can now produce a contents page and chapter openings without a
  pre-pass outside the engine.
- Layout cost: with a TOC, 2–3 full passes. The corpus and the CLI both use
  templates without a TOC, so their cost is unchanged.
- `toc.stable = false` is visible in the canonical bytes, so a browser or a
  gate can detect a non-converged contents page.
- New debt D73: no dot leaders, no per-level number formatting, no
  `ListOfFigures`/`Index` (only headings feed the TOC).

## Alternatives rejected

- **Generate the TOC in the CLI before layout.** It would need a layout pass to
  get page numbers anyway, and every other consumer (demo, eval, wasm) would
  have to duplicate it.
- **Two passes only.** A second pass can add a contents page that pushes the
  document by a page, which changes the numbers again; the fixed point is
  cheap and honest.
- **One flow item per box.** Rows would be split by the paginator.
- **Select the chapter master from the heading style.** Heading level is a
  semantic property, but "this page opens a section" is a pagination fact;
  resolving it in the paginator keeps styled data free of page state.
