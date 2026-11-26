# ADR-0081: A reference template transcribed from pandoc `soma.latex`

- Status: Accepted; promoted to the built-in default by ADR-0088
- Date: 2026-11-25
- Related: ADR-0003 (pure YAML template), ADR-0009 (semantic/presentation
  boundary), ADR-0014 (typographic refinements), ADR-0019 (table rules),
  ADR-0031 (alignment and cover), ADR-0069 (LaTeX article defaults),
  `docs/template.md`
- Touches: `examples/templates/plex.yaml`, `scripts/plex.sh`,
  `docs/template.md`, `docs/testing.md`

## Context

A user-supplied pandoc template at
`~/.local/share/pandoc/templates/soma.latex` defines a recognisable page
design: A4 with one-inch margins, IBM Plex Sans SC for CJK, Latin and code
alike, `\linespread{1.2}`, no first-line indent, a 20pt paragraph skip,
italic framed quotations, booktabs three-rule tables, and a `titling` title
block. The template is a design reference, not a source of engine behavior.

The engine needed a non-book example that exercises a different page geometry,
a sans body, zero indent with a large paragraph skip, a full-width A4 measure,
an unnumbered article hierarchy, and a pinyin index. Keeping the design as an
example template makes the correspondence inspectable and gives users a
starting point that is not the built-in `latex-article` defaults.

## Decision

Add `examples/templates/plex.yaml` as a reference page design with these
values:

| Element | Value | Reference |
|---|---|---|
| Page | A4, 25.4mm margins, 8mm footer, centred `{page}` | `geometry{top/inner/outer/bottom=1in}` |
| Body | IBM Plex Sans SC, 10pt, 1.44 line-height | `\setmainfont`, `\linespread{1.2}` on a 12pt baseline |
| Paragraph | `first-indent: 0`, `space-after: 20pt` | `\parindent 0em`, `\parskip 20pt` |
| Headings | bold, unnumbered, 14.4/12/10pt, no page break | `article.cls` `\section`, `\subsection`, `\subsubsection` |
| Quote | 1.5em side indent, italic, no first-line indent | `\renewenvironment{quote}{framed}\itshape` |
| Code | sans, 9pt, ragged, 1em around | `listings` `\ttfamily\small`, `frame=single` |
| Table | 0.8pt outer, 0.6pt header, no row or column rules | `booktabs` |
| Index | pinyin order, 9pt entries | Chinese publishing convention |
| Cover | title 25pt bold, author/date centred, no folio | `\maketitle`, `\droptitle{-8em}` |

`scripts/plex.sh` measures IBM Plex Sans SC in both family slots and the
bundled New Computer Modern Math face, then renders `examples/book/full.tex`
to `examples/templates/out/plex.pdf`. The font is not redistributed: the
script resolves a system copy with `fc-match`, and `SOMA_PLEX_REGULAR` /
`SOMA_PLEX_BOLD` override it.

The template is deliberately not the built-in default. The default remains
LaTeX `article` with a serif body, a 1.5em first-line indent and no paragraph
skip (ADR-0069).

## Consequences

- The reference design is reproducible from a fresh clone with the font
  installed; the generated PDF and metrics table stay out of git.
- A template test pins the design values (margins, 10/14.4pt, zero indent,
  20pt skip, 1.44 line-height, italic quote, booktabs rules), so an edit
  cannot silently drift from this record.
- Four capabilities of the pandoc template remain approximations:
  - a framed quotation becomes indent + italic; the template model has no
    border, background or frame property (ADR-0003 scope);
  - `listings`' frame is omitted; the code block is distinguished by family,
    size and spacing only;
  - `tikz`/`pgfplots`/`forest` are not template features and are not emulated;
  - the `titling` title block on page 1 becomes a separate cover page, because
    `cover:` is an atomic generated page.
- `tabularx` automatic column widths are not reproduced: table columns keep
  the engine's equal-share model (D90). A three-rule table still matches the
  reference's visible style.
- The reference's global `\XeTeXlinebreakskip` is not copied. CJK/Latin gaps
  remain rule-driven and are not user-tunable stretch glue.
- `\emergencystretch=3em` is an engine preset, not a template property
  (ADR-0009 §5); the sample uses the default emergency policy.
- IBM Plex Sans SC has no italic face and the PDF backend does not synthesise
  an oblique. `slant: italic` is declared and the HTML renderer honours it, but
  the PDF quote reads upright; a dedicated italic face or synthetic slant is a
  future feature.
- `fancyhdr`'s default running head is not reproduced. The engine places the
  folio in the footer, not the header, so the design uses `header: 0mm` and a
  centred footer folio.
- `\setmonofont` also points at Plex, so the sample does not demonstrate a true
  monospace face; a template that wants one selects `family: mono` and the
  caller passes `--font-sans`/`--font-sans-bold` for that file.

## Alternatives

- **Change the built-in `latex-article` template.** Rejected: the reference is
  a sans, zero-indent, large-skip design and would break every existing golden
  and the corpus baseline.
- **Add border/background to the template schema.** Rejected for this task: it
  is an architecture extension, not a template transcription, and would need
  its own ADR and layout work.
- **Bundle IBM Plex Sans SC.** Rejected: the font is not ours to redistribute;
  the script resolves a local install.

## Alignment

- `examples/templates/plex.yaml` — the design.
- `scripts/plex.sh` — metrics and PDF reproduction.
- `crates/soma-template/tests/a2_template.rs` — the design test.
- `docs/template.md` §11 — example and capability gaps.
- `docs/testing.md` — test count.
