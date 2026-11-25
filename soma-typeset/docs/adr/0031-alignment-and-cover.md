# ADR-0031: Alignment, generated cover, and the first release artifact

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0007 (frozen kernel), ADR-0028 (TOC and section-start masters),
  ADR-0029 (CLI), `docs/first-release.md`
- Touches: `soma-core` (accepts `Align::Left`), `soma-template` (`cover:`,
  alignment accepts all four values), `soma-layout` (line shifting, cover page)

## Context

Rendering the first complete book (`examples/book`) surfaced three gaps that no
unit test had caught, because each one is only visible when a designer's
template is applied to real content:

1. The template accepts `align: justify|left|center|right`, but the kernel
   rejects everything except `justify`, so any centred title failed at layout.
2. `meta.cover` has existed since A1 and nothing typesets it; a book without a
   cover is not a release artifact.
3. Table cells inherited `align: justify`, so a two-character cell was stretched
   to the column width (measured: 143%, an emergency line).

## Decision

### Left is a kernel alignment; centre and right are layout line shifts

`Align::Left` is now accepted by `break_paragraph`. The **breakpoints** still come
from the justified cost model (a line that fills the measure is still preferred),
but the boxes are placed at their natural widths: `adjustment = 0`.

`Center` and `Right` are rejected by the kernel and applied by `soma-layout`
after breaking:

```text
full  = line.target_width + line.indent
slack = full - line.natural_width
Center: shift = slack/2 - line.indent
Right:  shift = slack   - line.indent
```

Every atom in the line moves by the same amount, so the line keeps its internal
spacing and its trim. Overfull lines are left alone and overflow. This keeps the
frozen DP, cost model and tie-break untouched: alignment is a post-pass, and the
kernel never sees `Center`/`Right`.

Consequences worth stating: a centred paragraph breaks at exactly the same
points as the same paragraph justified, which makes the behaviour predictable
and testable; and `Line.adjustment` is `0` for both Left and centred lines, so
`hanging_punctuation` is skipped for them (it only makes sense for justified
lines).

### The cover is generated content, owned by the layout

Template:

```yaml
cover:
  master: cover      # optional; defaults to the document master
```

The four lines come from the document: `meta.cover` when present
(`title`/`subtitle`/`authors`/`date`), otherwise `meta.title`, `meta.authors`
and `meta.date`. Authors join with `、` for Chinese documents and `, ` otherwise.

Styles: `element: coverTitle`, `coverSubtitle`, `coverAuthor`, `coverDate`, all
inheriting from `document`. The template decides *that* a cover exists and how it
looks; the document decides *what* it says.

The cover is **one atomic flow item**: all four lines are blocks of a single
item, with spacing applied inside the item (the title's `space-before` positions
the cover, it is not page glue, which the paginator discards at a page top). A
cover that does not fit therefore marks the page `overflow` instead of spilling
onto a second page. The cover is followed by a forced break.

`\subtitle{...}` in the LaTeX frontend fills `meta.cover.subtitle`; there is no
other DocIR field for it.

### Schema

`soma.template/7` (adds `cover`), `soma.styles/4` (adds the cover style ids and
`cover`). The page result is unchanged (`soma.pages/7`): a cover is just blocks.

## Consequences

- A template can centre titles, cover lines and captions without an
  `UnsupportedAlign` failure, and `align: left` is usable for cells.
- The example book typesets cover + contents + six chapters with 0 overflow and
  0 degraded lines; `crates/soma-cli/tests/cli.rs` renders it on every run.
- New debt: D75 (selectors cannot see heading level), D76 (no front-matter page
  numbering), D77 (no automatic chapter numbering).
- `Align::Left` in the kernel is a *feature*, not a bug fix, so this ADR also
  records that the frozen-kernel invariant (ADR-0007) is about the DP, cost
  model and canonical line shape — those did not change.

## Alternatives rejected

- **Make the kernel implement ragged breaking (a different cost model).**
  Changes the DP, the golden results and the calibration; ragged layout as a
  post-pass is what LaTeX's `\raggedright` effectively does to the boxes anyway.
- **Add `align` to `PlacedBlock` and let renderers shift lines.** Renderers must
  not compute geometry (ADR-0010); the canonical result would no longer describe
  where atoms are.
- **Document-driven cover presence.** A cover is a design decision: a template
  without `cover:` must not grow one because the source happens to set a title.
- **Let the cover spill across pages.** A two-page cover is always a bug; an
  explicit `overflow` flag is the honest failure.
