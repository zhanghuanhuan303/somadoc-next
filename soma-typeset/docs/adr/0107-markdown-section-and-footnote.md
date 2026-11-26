# ADR-0107: Markdown lifts LaTeX section commands and inline footnotes

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0104 (compound Markdown frontend), ADR-0008 (DocIR), D109
- Touches: `soma-md` (`embedded.rs`, `lib.rs`, `tests/mixed.rs`),
  `docs/capabilities.md`, `docs/debt.md`

## Context

ADR-0104 made Markdown a compound frontend: inline LaTeX commands and a
whitelist of block environments already reach DocIR. A real mixed document
still failed in two places:

- a line beginning with `\section{...}` was treated as Markdown prose. The
  backslash escape dropped the command name and the reader saw
  `section{脚注与索引}` instead of a heading;
- `\footnote{...}` was deliberately kept as `RawInline` (D109), so the source
  text appeared in the page and its argument was never a footnote definition.
  An `\index` inside that argument was therefore also invisible to the index.

This is not a rendering problem: the Markdown frontend never produced the
semantic nodes.

## Decision

### Block sectioning commands

At the start of a block, a complete LaTeX sectioning command
(`\section`, `\subsection`, `\subsubsection`, `\chapter`, `\paragraph`,
`\subparagraph`) whose argument ends the line is delegated to the LaTeX
frontend. Its blocks are appended in order; the first must be a `Heading`, or
the line falls back to ordinary Markdown. The LaTeX levels are unchanged:
`\section` is level 1, `\subsection` level 2, and so on.

The scanner recognises the command, including its brace group, but not an
optional `*`/`[...]` argument. A line with trailing Markdown after the command
is not a block command; it stays a paragraph, where an inline sectioning
command is preserved as `RawInline` rather than silently dropped.

### Inline footnotes

The inline scanner still keeps `\footnote{...}` as `RawInline` while parsing
the paragraph, because the brace group may contain arbitrary LaTeX and
Markdown text that must not be reparsed by the Markdown scanner. After the
whole document is parsed, `lift_latex_footnotes` walks every block:

1. a `RawInline` whose format is `latex` and whose value starts with
   `\footnote` is handed to `soma_latex::parse_latex_inline_fragment`;
2. the returned `FootnoteRef` replaces the raw inline; the returned
   `FootnoteDef` is relabelled `latex-note-N` so it cannot collide with a
   Markdown `[^label]` definition;
3. references in the lifted fragment are rewritten to the new label;
4. the pass recurses through paragraphs, headings, list items,
   block quotes, figure/table captions, table cells and generated blocks, so
   a footnote works in a list or a table cell too.

Because the LaTeX frontend parses the footnote body, `\index` inside it
becomes an `IndexRef` and is collected on the page that holds the reference.
The same pass closes the footnote part of D109. Preamble macros and counters
in Markdown (`\newcommand`, `\newcounter`) remain unsupported.

## Consequences

- A Markdown document may mix `# Markdown`, `\section{LaTeX}`, inline
  `\footnote`, `\index`, `\texttt` and HTML without leaving command source on
  the page.
- Delegated blocks carry synthesized `NodeMeta`; the source span of a lifted
  footnote refers to the whole document, not to the inner note text.
- Public `soma_md::parse_inlines` still keeps `\footnote` as raw because it has
  no document to hold a definition; only full-document parsing lifts notes.
- A section command at the start of a paragraph with trailing text is not
  promoted; this avoids absorbing Markdown prose into a LaTeX title.
