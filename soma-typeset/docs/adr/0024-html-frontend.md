# ADR-0024: HTML frontend (semantic whitelist)

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0004 (syntax frontends), ADR-0009 (semantics/presentation),
  `docs/architecture-v2.md` §3, `docs/debt.md` D24/D53
- Adds: `crates/soma-html` (`html` → DocIR). No layout or schema change.

## Context

DocIR has been designed for multiple frontends since A1, but only Markdown was
implemented (D24). The PRD's pipeline starts from "Markdown/HTML/LaTeX/Word,
whichever the author has"; without an HTML frontend, HTML documents cannot use
the engine at all.

The architecture already fixes the shape of this frontend: a semantic
whitelist, no CSS layout, no scripts, unknown tags degrade instead of failing,
and the frontend knows nothing about templates or pages.

## Decision

### A hand-written tokenizer and a tiny tree

`soma-html` is zero-dependency, `no_std + alloc`, and `#![forbid(unsafe_code)]`,
matching `soma-md`. It tokenizes tags, attributes, comments, doctypes, raw-text
elements (`script`, `style`), void elements and character references, then
builds a small element tree with tolerant recovery:

- unclosed elements are closed at EOF;
- stray closing tags are ignored;
- `<p>`, `<li>`, `<tr>`, `<td>/<th>` auto-close on the next sibling;
- `<script>`/`<style>` content is dropped entirely.

Attributes are not carried wholesale. Only `class`/`role`/`id` become DocIR
`Attrs` (role, classes, anchor); `href` feeds links, `src`/`alt` feed images,
`colspan`/`rowspan` feed table cells, `lang` on `<html>` fills the document
language, and `start` feeds ordered lists.

### Supported semantics

`h1`–`h6`, `p`, `strong`/`b`, `em`/`i`, `u`, `s`/`del`/`strike`,
`code`/`kbd`/`samp`/`var`, `a`, `img`, `br`, `sup`, `sub`, `ruby`/`rt`,
`ul`/`ol`/`li`, `blockquote`, `pre`, `hr`, `table`/`thead`/`tbody`/`tr`/`th`/
`td`, `figure`/`figcaption`, and transparent containers (`div`, `section`,
`article`, `main`, `header`, `footer`, `nav`, `aside`, `body`, `html`).

Tables are normalized into the DocIR grid: the column count comes from the
widest row after spans, rows are padded with empty cells, spans are clamped to
the grid, and leading `thead`/all-`th` rows become `Row.header`. This keeps
every parsed table valid under `Document::validate`.

Whitespace follows HTML: runs collapse to one space inside normal flow and are
preserved in `pre`. Paragraph boundaries trim the edge spaces. Entities cover
the common named set plus decimal and hexadecimal references.

Unknown tags are transparent: their children are kept, and text is never
dropped. There is no diagnostics channel in v1; the capability list
(`SUPPORTED_TAGS`) and the tests are the contract.

## Consequences

- The PRD's second syntax is usable end to end: HTML → DocIR → template →
  pages → HTML/SVG, with the same canonical and determinism guarantees as
  Markdown.
- `scripts`/`style` never reach DocIR, so Gate C's "no scripts or URLs from
  content" promise continues to hold at the renderer.
- D24 narrows to LaTeX/Word; a new D69 records HTML subset limits (no CSS,
  forms, frames, footnotes, `<caption>`/`<colgroup>`, and only a small entity
  set).
- No template, layout, engine, or page schema changes.

## Alternatives rejected

- **A full HTML5 parser dependency (`html5ever`).** The workspace is offline,
  no_std-friendly, and the frontend is a whitelist; a full parser would also
  accept trees DocIR cannot represent, requiring a second degradation layer.
- **Emitting `RawInline` for unknown tags.** Keeping the children preserves
  author content without inventing a raw payload the renderer must escape.
- **Mapping `style="..."` to template attributes.** That would let content
  smuggle presentation into layout, which ADR-0009 explicitly forbids.
