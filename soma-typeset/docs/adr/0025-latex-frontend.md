# ADR-0025: LaTeX frontend (subset)

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0004 (syntax frontends), ADR-0009 (semantics/presentation),
  `docs/debt.md` D24/D53/D69
- Adds: `crates/soma-latex` (`latex` → DocIR). No layout or schema change.

## Context

DocIR is designed for several syntax frontends. Markdown and HTML are
implemented; LaTeX is the third source the PRD names, and many Chinese
academic/technical documents start as `.tex`.

A full TeX engine is out of scope (and would duplicate the layout core). What
the pipeline needs is a **semantic reader**: headings, paragraphs, emphasis,
lists, quotes, code, tables, math *source*, footnotes, and basic metadata.

## Decision

### Hand-written reader, zero dependencies

`soma-latex` is `no_std + alloc`, `#![forbid(unsafe_code)]`, and depends only on
`soma-doc`, matching `soma-md`/`soma-html`. It is a recursive-descent reader
over the source text:

- comments are stripped (`\%` stays);
- the body starts at `\begin{document}` (if present);
- `\title`, `\author` (split on `\and`), `\date` (ISO `YYYY-MM-DD`) fill the
  metadata when the caller did not set it;
- unknown commands are transparent: a braced argument is kept as text, a bare
  command is dropped;
- unterminated groups and environments keep the remaining text instead of
  failing.

### Supported semantics

- `\section`/`\subsection`/`\subsubsection`/`\paragraph`/`\subparagraph` →
  `Heading` levels 1–5 (starred variants accepted).
- Paragraphs separated by blank lines; `\par`; `\\`/`\newline` → hard breaks.
- `\textbf`, `\emph`/`\textit`/`\textsl`, `\underline`, `\textsc`,
  `\textsuperscript`, `\textsubscript`, `\texttt`, `\verb`.
- `\href{url}{text}`, `\url{url}` → links; `\footnote{...}` → a footnote
  definition plus a marker.
- Inline `$...$` / `\(...\)` and display `\[...\]` / `$$...$$` / `equation`
  environments → `MathInline`/`MathBlock` with the source kept verbatim.
- `itemize`/`enumerate`, `quote`/`quotation`, `center`/`flushleft`/`flushright`,
  `verbatim`, `tabular` (rows by `\\`, cells by `&`, booktabs rules,
  `\multicolumn{n}{..}{..}`), and `\newpage`/`\clearpage`.

Tables normalize into the DocIR grid: the widest row sets the column count,
rows are padded with empty cells, spans are clamped, and `\midrule` or a
post-row `\hline` ends the header. Cell content is plain text in v1.

## Consequences

- The PRD's third source is usable end to end: LaTeX → DocIR → template →
  pages → HTML/SVG.
- D24 narrows to Word. A new D70 records the subset limits (no macros,
  counters, bibliography, `\includegraphics`, real math typesetting, cell
  markup, or `\item[label]`).
- Math is stored, not laid out: this frontend is the producer that a future
  `soma-math` adapter will consume.
- No template, layout, engine, or page schema changes.

## Alternatives rejected

- **Embedding a TeX engine.** It would bypass DocIR and the deterministic page
  model, and tie the frontend to the frozen kernel.
- **Dropping unknown commands with their arguments.** Loses author text;
  transparent arguments keep content without pretending to understand it.
- **Mapping `\usepackage`/macros to template features.** Presentation belongs
  to the template layer (ADR-0009), not to source commands.
