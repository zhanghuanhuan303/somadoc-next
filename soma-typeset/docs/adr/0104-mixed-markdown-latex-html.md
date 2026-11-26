# ADR-0104: Markdown is a compound frontend for embedded LaTeX and HTML

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0008 (DocIR), ADR-0102 (diagram compiler), ADR-0092 (TikZ),
  ADR-0103 (Studio TikZ export)
- Touches: `soma-md` (mixed parsing, new dependencies on `soma-html` and
  `soma-latex`), `soma-html` (`parse_html_fragment`), `soma-latex`
  (`parse_latex_inline_fragment`), `scripts/gate-a.sh`, `docs/capabilities.md`,
  `docs/architecture-v2.md`, `examples/mixed/`

## Context

Markdown is the primary authoring syntax, but real documents routinely mix it
with:

- inline and display mathematics (`$...$`, `$$...$$`, `\(...\)`, `\[...\]`);
- LaTeX text commands (`\textbf`, `\emph`, `\underline`, `\href`, `\url`,
  `\ruby`, `\index`) and block environments (`itemize`, `quote`, `tabular`,
  `figure`, `equation`, `tikzpicture`, `mermaid`);
- semantic HTML (`<u>`, `<b>`, `<a>`, `<img>`, `<sup>`, `<sub>`, `<ruby>`) and
  HTML block containers such as `<div>`, `<table>` and `<ul>`.

Before this decision, Markdown kept every HTML tag as `RawBlock`/`RawInline`
fallback text and had no math extension at all. That was safe but meant a
document mixing three syntaxes was rendered as source, not as the semantics the
author wrote. The architecture already says every frontend produces the same
DocIR; this ADR makes Markdown a **compound frontend** that delegates to its
peers instead of reimplementing them.

## Decision

### Dependencies

`soma-md` may depend on `soma-html` and `soma-latex`; both still depend only on
`soma-doc`. There is no cycle and no new lower-layer dependency. `gate-a.sh`
records this edge explicitly. The alternative of copying either parser into
`soma-md` was rejected because it would create two semantic mappings that drift.

### Inline math

- `$...$` and `\(...\)` become `MathInline`.
- `$$...$$` inside a paragraph becomes `MathInline`; at the start of a block it
  becomes `MathBlock`.
- A dollar run is only math when the opening is not followed by whitespace, the
  closing is not preceded by whitespace, and the closing is not immediately
  followed by a digit. This keeps `价格 $5 和 $10` as text.
- Escaped `\$` remains literal text.

### Block math and LaTeX environments

At the start of a block, `$$...$$`, `\[...\]`, and a whitelist of LaTeX
environments are handed to the LaTeX frontend. The whitelist covers math
(`equation`, `align`, `gather`, `multline`, `displaymath`), lists, quotations,
`tabular`, `table`, `figure`, `verbatim`, `center`/`flush*`, `tikzpicture`, and
`mermaid`/`dot`/`graphviz`. An environment without a closing token stays
ordinary Markdown text instead of swallowing the rest of the document.

The resulting blocks are appended to the same `Document`; node ids are assigned
once by the Markdown document. Footnote labels from an embedded LaTeX block are
merged only when they do not collide with a Markdown definition; a collision
falls back to a visible `RawBlock`, never an unresolved reference.

### Inline LaTeX commands

A whitelist of commands is recognized only when every required brace argument is
present:

- containers: `\textbf`, `\emph`, `\textit`, `\textsl`, `\underline`,
  `\textsc`, `\textsuperscript`, `\textsubscript`, `\texttt`;
- links/code: `\href`, `\url`, `\verb`;
- DocIR extensions: `\ruby`, `\index`;
- symbols: `\ldots`, `\dots`, `\LaTeX`, `\TeX`, `\newline`;
- metadata commands `\label`, `\cite`, `\ref`, `\pageref` consume their key
  like the LaTeX frontend does.

The command source is passed to `soma_latex::parse_latex_inline_fragment`, so
nested commands and math inside arguments follow the LaTeX parser, not a second
implementation. `\footnote` is recognized only to preserve its source visibly;
inline LaTeX footnote definitions are not merged (D109). Unknown commands and a
command with a missing argument keep Markdown's existing text behavior.

### Inline HTML

Formatting tags map to DocIR containers: `strong`/`b`, `em`/`i`, `u`/`ins`,
`s`/`del`/`strike`, `sup`, `sub`, `code`/`kbd`/`samp`/`var`, `a`, `img`, `br`,
and `ruby`/`rt`. Transparent tags (`span`, `small`, `mark`, `cite`, `q`, `abbr`,
`time`) keep their children. Children are parsed as Markdown recursively, so
`<u>下划线 **粗**</u>` produces `Underline(Strong(...))` and embedded math still
works.

Attributes are scanned with quoted values, and common named/numeric entities are
decoded. `href`/`src`/`alt`/`title` are carried into `Link`/`ImageRef`. Unknown
tags remain `RawInline` with visible source. `<script>`, `<style>` and CSS
layout are never executed.

### Block HTML

A supported root tag (`div`, `section`, `p`, headings, lists, blockquote, pre,
table, figure, etc.) is collected through its matching close tag, including
blank lines, and parsed by `soma_html::parse_html_fragment`. Text nodes in the
result are then parsed as Markdown inline syntax, so math and LaTeX inside an
HTML paragraph work without a second HTML parser. The pass disables HTML
recognition, so a decoded `&lt;b&gt;` remains literal `<b>` rather than becoming
a second parse.

Unsupported roots (for example `<script>` or `<iframe>`) keep the previous
visible `RawBlock` fallback. HTML is never shown as executed output and the
renderers still emit no active content.

### Capabilities and tests

`soma-md` grows four capability rows: `inline_math`, `display_math`,
`latex_inline`, `latex_block`; the matrix is now 34 entries. `html_block` and
`html_inline` keep their `supported` status but now describe semantic mapping
instead of raw-only fallback. Tests cover currency heuristics, nested Markdown
inside HTML, HTML entities, LaTeX commands/environments, tables, TikZ/Mermaid,
and a full mixed sample.

## Consequences

- One source can mix Markdown, LaTeX and HTML and still produce one deterministic
  DocIR and one rendered page tree. The Studio, CLI, PDF, SVG and HTML renderers
  need no syntax-specific branch.
- `soma-md` now links two peer frontends. The core crates remain dependency-free;
  the cost is binary size and a slightly larger parser test surface.
- Unknown syntax remains visible and no content is dropped. Inline LaTeX
  footnote definitions and LaTeX counters/macros defined in a Markdown preamble
  are not supported (D109).
- HTML block text is intentionally parsed as Markdown inline syntax. Block-level
  Markdown structures inside an HTML container (for example a Markdown list
  inside `<div>`) remain HTML content or text; only the HTML frontend decides
  block structure.

## Alternatives

- **Keep HTML and LaTeX as raw fallback.** Rejected: mixed documents render as
  source and the user's main authoring workflow is lost.
- **Reimplement math and HTML in `soma-md`.** Rejected: duplicate semantic
  mappings and drift.
- **Pass the whole Markdown document through the HTML parser first.** Rejected:
  Markdown is not HTML; headings, lists and fenced code would be lost or
  reinterpreted.
- **Parse Markdown blocks inside every HTML element.** Deferred: it would need a
  block-level grammar switch inside the HTML tree and a clear ownership rule for
  blank lines. Inline syntax is enough for the current mixed documents and is
  recorded as a boundary.
