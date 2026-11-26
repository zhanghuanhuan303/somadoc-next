# ADR-0069: the default template is LaTeX article 10pt

- Status: Superseded by ADR-0088 (the design lives on as
  `examples/templates/latex-article.yaml`)
- Date: 2026-11-21
- Related: ADR-0015 (page masters), ADR-0023 (`publish-v1`), ADR-0029 (CLI),
  D13 (`emergency_stretch` has two sources)
- Touches: `soma-cli` (built-in template, default preset), `soma-core`
  (`latex-v1`), `soma-engine` (`preset: latex-v1`), `soma-layout` (margin
  bands)

## Context

`soma render` without `--template` used `soma-default`: A4, 20 mm margins,
10.5 pt / 1.6, a 2 em first-line indent, no folio, and no numbering. Without
`--engine` it used `publish-v1`. That is the engine's own house style, not a
reference a reader already knows: a first page that looks odd cannot be
attributed to the engine or to an unfamiliar default.

LaTeX's `article` class is the most widely known baseline for Western
technical documents and the layout many Chinese authors have seen. Its 10 pt
A4 geometry is computed by the class rather than tabulated, so the numbers
were measured from `pdflatex` on this machine instead of copied from a
textbook:

```text
\documentclass[10pt,a4paper]{article}
\textwidth = 345pt   \textheight = 598pt
\oddsidemargin = 53pt   \topmargin = 17pt
\headheight = 12pt   \headsep = 25pt   \footskip = 30pt
\parindent = 15pt   \baselineskip = 12pt   \parskip = 0pt plus 1pt
```

Note that `a4paper` keeps the 345 pt measure but raises the height from the
letter-paper 550 pt to 598 pt, so the often-quoted "345 x 550 pt" pair is
wrong for A4.

## Decision

### 1. The built-in template is `latex-article`

The text block is placed by four margins, because the template has no
`\topmargin`/`\oddsidemargin` offsets:

| Edge | TeX origin | Value |
|---|---|---|
| left | `1in + \oddsidemargin` = 125.27 pt | 44.0 mm |
| right | `\paperwidth - left - \textwidth` = 127.24 pt | 44.7 mm |
| top | `1in + \topmargin + \headheight + \headsep` = 126.27 pt | 44.4 mm |
| bottom | `\paperheight - top - \textheight` = 120.78 pt | 42.4 mm |

The resulting block is 121.3 x 210.2 mm, i.e. 345.0 x 598.1 TeX points after
the 0.1 mm rounding of the margins. `footer: 10.5mm` is `\footskip`; the
footer band therefore ends a folio 30 pt below the text block. `\headheight`
and `\headsep` are folded into the top margin and `header: 0` means no running
head, which is what `article`'s empty `\pagestyle{plain}`-style head does.

Styles follow the class defaults:

| Element | LaTeX | Template |
|---|---|---|
| body | 10 pt / 12 pt | `font-size: 10pt`, `line-height: 1.2` |
| `\parindent` | 15 pt = 1.5 em | `first-indent: 1.5em` |
| `\parskip` | 0 pt | `space-before/after: 0` |
| `\section` | `\Large\bfseries`, 3.5ex / 2.3ex | 14.4 pt bold, 1.5em / 1em |
| `\subsection` | `\large\bfseries`, 3.25ex / 1.5ex | 12 pt bold, 1.4em / 0.65em |
| `\subsubsection` | `\normalsize\bfseries` | 10 pt bold, same spacing |
| `\footnotesize` | 8 pt / 9.5 pt | 8 pt, `line-height: 1.1875` |
| `\small` captions | 9 pt | 9 pt, centred |
| `itemize` | `\leftmargini` 2.5 em, `\itemsep` 4 pt, `\topsep` 8 pt | 2.5 em total, 0.4 em between items |
| `quote` | 2.5 em both sides, no indent | 2.5 em both sides, `first-indent: 0` |
| `verbatim` | `\ttfamily` | `family: mono` |

`mono` selects the second family, because the renderer has exactly one
non-body face; a host with a monospace face passes it as `--font-sans`.

### 2. The default preset is `latex-v1`

`ParamSet::latex_v1()` is `default-v1` with two changes:

- `tolerance = 1260` per-mille. TeX's tolerance is a badness limit and our
  badness is `100 * |r|^3`, so `\tolerance=200` means `|r| <= 2^(1/3) =
  1.2599`. The current engine default `publish-v1` allows `|r| <= 1.5`, which
  is looser than the class.
- `short_last_scale = 0`. TeX has no short-last-line penalty; a short final
  line is free, and only `\parfillskip` keeps it from collapsing.

The rescue ladder is unchanged. TeX sets `\emergencystretch=0pt` and would
emit an overfull box where we stretch one line in the rescue pass; the rescue
is not part of the normal path and its 3 em stretch is worth keeping.

`preset: latex-v1` is accepted by `soma.engine/1`. An explicit `--engine`
file and an explicit `--template` keep their own choices; only the built-in
defaults change.

### 3. A margin band owns its line

`margin_block` used to place both a header and a footer line at the top of the
band with `line_height = band_height`. Now the block still spans the band, but
the line keeps its natural line box: a header line sits at the top of the band
and a footer line at the bottom, so a folio lands `\footskip` below the text
block instead of touching it.

`ParagraphStyle::line_height` cannot be zero — DocIR rejects it — and it is
also the reference body height of the K3 height cost. A margin block therefore
uses `Metrics::line_metrics(style).height()`, its natural line box.

## Consequences

- The canonical page output of `soma render` without flags changes: different
  margins, different line breaks, different page count, and a folio.
- The SVG byte hash (Gate R) changes, because footer lines move inside their
  band. Body coordinates are untouched; the new hash is annotated as a
  deliberate visual change.
- Templates that ship with the examples (`book.yaml`, `full.yaml`,
  `song.yaml`) and the corpus scripts pass `--template` and are unaffected.
- `docs/testing.md`, `examples/book/brief.tex`, and the test counters move by
  the four tests added with this change.

### What is deliberately not copied

- `\topskip` and `\parskip = 0pt plus 1pt`. Our paragraph spacing is fixed;
  no class relies on that 1 pt of stretch in practice.
- `\@seccntformat`'s 1 em gap after a heading number. Numbered prefixes use one
  space, and changing that would change every caption and heading.
- `\raggedbottom`/`\flushbottom` and the `\baselineskip` glue. The vertical
  breaker owns page fill.
- `\ttfamily` as a real typewriter face: the PDF backend has one secondary
  family, so `mono` maps to it.

## Alternatives

- **Keep `soma-default`.** Rejected: an unknown house style hides whether a
  bad line is the engine's fault.
- **Copy the textbook 345 x 550 pt block.** Rejected: `a4paper` computes 598 pt
  of height, and the class is the reference we claim to follow.
- **Use the `geometry` package's values.** Rejected: that is a different
  package with different defaults; `article` alone is the recognisable one.
- **Allow `line_height = 0` for margin blocks.** Rejected: DocIR's invariant
  keeps every paragraph measurable, and the K3 height cost divides by it.
