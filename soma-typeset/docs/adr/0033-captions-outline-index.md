# ADR-0033: Caption styles, wrapping contents rows, outlines, and indexes

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0028 (TOC), ADR-0032 (numbering, front
  matter, TOC lists), `docs/debt.md` (D78, D79)
- Touches: `soma-doc` (`Inline::Index`, `soma.doc/2`), `soma-template`
  (`figureCaption`/`tableCaption`/`indexEntry`/`indexTitle`, `index:`),
  `soma-layout` (caption styles, wrapping TOC rows, outline, index pass),
  `soma-md` (`[[index:term]]`), `soma-latex` (`\index{term}`)

## Context

The first ten-page book exposed four long-document gaps that the previous rounds
deferred:

1. Captions inherited the block they belonged to, so a figure caption was
   justified and first-indented like body text (D78).
2. Contents and index rows were single object runs: a title wider than its box
   could overflow but could not wrap (D79).
3. Nothing in the result described the document's navigation structure. A PDF
   backend had to re-read DocIR and re-invent the page mapping to write
   bookmarks.
4. There was no index. `\index{...}` was explicitly dropped by the LaTeX
   frontend.

The constraints are the same as before: DocIR stays semantic, generated content
is produced by layout, every generated string enters the canonical hash, and the
fixed-point driver must not grow a second loop.

## Decision

### Captions are generated elements

`figureCaption` and `tableCaption` join `ELEMENTS`. Caption inlines resolve
under their generated element, so `style_of(caption_text)` already carries the
caption font, alignment and indent; emphasis nested inside the caption inherits
from the caption, not from the body. Layout uses the interned caption style as
the paragraph style of the generated caption.

Table captions are emitted **above** the table with `keep-with-next`, which is
the usual Chinese book convention; figure captions stay below the figure. A
caption is still generated content: its text comes from the document, its
number (when `numbering:` asks for one) comes from layout.

### Contents rows wrap; the number follows the last line

TOC and index rows now compile their title with `SpanKind::Text` instead of one
object atom. A long title breaks inside its box. The page-number box and the
leader run are placed at the **last line's** `y`, so a two-line entry reads

```text
第一章 一个很长、需要换行的标题……
                                      ····· 7
```

Leaders are emitted only when the last line leaves room for them; a full last
line simply omits them. This changes no schema field — it changes the geometry
of an existing row.

### The outline is data, not a PDF writer

`PagedLayout.outline` carries one entry per heading in document order:

```json
{"level":1,"text":"第一章 为什么是全局优化","page":3,"printed":"1"}
```

`page` is the physical page, `printed` is the label a reader sees. A PDF backend
can build the bookmark tree from `(level, text, page)` without re-reading the
document; HTML can use the same list for navigation. Writing an actual PDF
outline remains part of the PDF renderer (N2), which this project still does not
have.

### Index markers live in DocIR; the index itself is generated

DocIR gains one empty inline:

```rust
Inline::Index(IndexRef { meta, term })
```

It occupies no space, contributes no text, and is validated (non-empty term).
`soma.doc/2` is the schema bump. Frontends map their own syntax:

| Frontend | Syntax |
|---|---|
| LaTeX | `\index{排版}` |
| Markdown | `[[index:排版]]` (documented extension) |

Sorting and display strings are deliberately **not** in DocIR: the same marker
can appear in a book sorted by pinyin and in a report sorted by code point.

Template:

```yaml
index:
  title: 索引        # optional; omit or "" for an untitled index
```

Layout collects `(term, flow item)` while compiling the body, maps each item to
its printed page label after pagination, then:

- sorts terms by Unicode scalar value (deterministic, locale-free);
- merges repeated terms and lists each page label once, in occurrence order;
- emits one leader row per term at the tail of the flow, on a new page;
- uses `indexEntry` / `indexTitle` for the generated rows.

The index is appended **after** the body, so adding it cannot move the body
pages it refers to. That makes the pass converge in two iterations: the first
pass collects, the second renders the entries found by the first. The driver
compares headings, floats and index entries together and reports
`index: {passes, stable, entries}`.

Limits, documented rather than hidden: markers inside table cells and footnote
definitions are not collected (their flow item is not a top-level item); there
are no `see`/`see also` cross-references; sorting is code-point order, not
pinyin or stroke count.

### Schema

- `soma.doc/2`: `Inline::Index`.
- `soma.template/9`: `figureCaption`/`tableCaption`/`indexEntry`/`indexTitle`
  elements and the `index:` section (template 8, from ADR-0032, is the previous
  version).
- `soma.styles/7`: four new interned style ids.
- `soma.pages/9`: `index` report and `outline` list.

## Consequences

- The example book is 11 pages: cover, contents (chapters, sections, figure and
  table lists), eight body pages, one index page; 7 index entries; 9 bookmarks;
  0 overflow and 0 degraded lines.
- A template can style captions and the index independently of body text.
- Long chapter titles no longer overflow the contents box.
- Adding an index costs one extra layout pass only when markers exist.
- New debt: D80 (no pinyin/stroke collation), D81 (no index markers in cells or
  notes, no cross-references), both recorded in `docs/debt.md`.

## Alternatives rejected

- **Put caption styles in the float block's style.** A figure and a table in the
  same chapter would share one style, and a caption would inherit body indent
  and justification — the exact bug D78 recorded.
- **Keep contents rows as object atoms and clip them.** Clipping hides text;
  wrapping is what a reader expects and what the kernel already supports.
- **Put the page number at the top of a wrapped row.** The number would point at
  the first line while readers scan the last line, and leaders would cross the
  title.
- **Store outline entries in DocIR.** The page number is a layout result; DocIR
  must stay source-stable.
- **Generate the index in a frontend.** Three frontends would sort differently,
  and none of them knows the printed page labels.
- **Sort by pinyin.** Requires a pronunciation table or a dependency; the
  template decides typography, not collation. Recorded as D80 so a later
  `index-sort` template field can be added without changing DocIR.
- **Run a separate fixed-point loop for the index.** The existing loop already
  compares the full generated set; a second loop would double the pass cost and
  could oscillate between the two.
