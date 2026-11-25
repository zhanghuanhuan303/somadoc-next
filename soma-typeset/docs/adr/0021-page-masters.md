# ADR-0021: Named page masters selected by sections

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0015 (page masters: header, footer, folio), `docs/debt.md` D62
- Extends: the template schema (`soma.template/5`), `soma-layout` page
  assembly, and `Section.page_template`.

## Context

ADR-0015 gave every page the same header band, footer band, running title and
folio. DocIR has carried `Section.page_template` since A1 and validation
accepted it, but layout ignored it: a book could not say "no folio on the cover
page", and a chapter could not use a different running header.

Page masters that change the **content box** would change pagination capacity
per page, which the vertical DP does not model. The margin bands, however, live
outside the content box (ADR-0015), so masters can vary freely without touching
the page breaker.

## Decision

### Template: a named `masters:` list

```yaml
page:                 # master 0, named "default"
  header: 6mm
  footer: 6mm
  header-text: running-title
  folio: "{page}"
masters:
  - name: chapter
    header: 4mm
    footer: 0
    header-text: 章节页
  - name: cover
    header: 0
    footer: 0
    folio: null
```

- `name` is a token, required, unique, and `default` is reserved.
- A master states its own `header`, `footer`, `header-text` and `folio`; there
  is no implicit fallback to the default master (the template DSL avoids
  hidden inheritance, as with table rules).
- `header`/`footer` must be non-negative and must fit inside the page margins;
  the same check the default page already had now runs for every master.
- `resolve` produces `ResolvedMaster` values (em units) with master 0 being the
  default; `StyledDoc::master_index(name)` and `master(index)` expose them.

### Sections select a master, and start a new page

`Block::Section` with `page_template: Some(name)` looks the master up. An
unknown name is a layout error (`unknown page master`). When the master
changes, the builder inserts a zero-height forced break before the section's
title, so the page that begins the section is the first page with the new
master; a page never mixes two masters because the master comes from its first
item. A section at the very start of the document applies to page 1 without a
leading blank page.

`FlowItem` and `VItem` carry the master index; page assembly reads the first
item of the page and uses that master's bands, running text and folio.

### No geometry change

Masters only carry margin-band presentation. Every master shares the document
content box, so pagination, `usedHeight` and the page canonical bytes are
unchanged by choosing a master. A cover page with a smaller text block is still
D62's open part.

## Consequences

- `soma.template/5` adds `masters`; `soma.styles/2` now includes the resolved
  masters (it is the layer that owns resolved presentation), so `styleHash`
  changes for every template. `soma.pages/4` is unchanged: masters select which
  header/footer blocks are emitted, and those were already in the page arrays.
- `Section.page_template` finally has an effect; D62 narrows to per-master
  geometry (content box) and automatic cover generation.
- An unknown master name fails loudly instead of silently using the default.

## Alternatives rejected

- **Per-page capacity from masters.** The vertical DP would need capacity as a
  function of the page number, which changes the recurrence and the pruning
  window; that is a different model version, not a presentation feature.
- **A magic `first` master applied to page 1.** Explicit sections are
  deterministic and work for covers, chapter openers and appendices with the
  same mechanism.
- **Implicit inheritance from the default master.** The DSL already rejects
  implicit fallbacks; explicit keys keep the resolved style easy to read.
