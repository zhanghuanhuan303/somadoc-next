# ADR-0106: Book layout pass — chapter flow, running heads, segment tails

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0033 (TOC rows), ADR-0058 (hanging indent),
  ADR-0090 (code panel), ADR-0105 (ragged opening trim), D110
- Touches: `soma-template` (`template.rs`, `style.rs`, `canonical.rs`),
  `soma-layout` (`lib.rs`, `pagination.rs`), `soma-core` (`params.rs`),
  `soma-engine` (preset list), `soma-latex` (preamble metadata),
  `examples/book/song.yaml`, `scripts/full.sh`

## Context

The full-capability sample (`examples/book/full.tex`) is a book-shaped
document. Reviewing its 14 A5 pages exposed five structural problems that are
not K-P kernel problems:

1. The vertical K-P balanced the last two pages of a chapter, leaving two
   half-empty leaves (printed 10 and 11) instead of filling the first and
   letting only the chapter's last page be short.
2. `running-title` took the last heading of any level. A page could therefore
   be headed by a section (`第三节`) or by a chapter that had already ended on
   the previous page.
3. The sample build used `sed` to turn every `page-break-before: true` into
   `false`, so "continuous chapters" was a build-script side effect rather
   than a template decision.
4. A two-character last line (`定。`, `开。`) was accepted because the article
   preset deliberately follows TeX's ragged last line and charges nothing.
5. Several visual roles had no independent style: running heads inherited the
   body alignment (and previously only centred by accident), TOC rows had one
   style for every level, and a figure could only be full-width.

## Decision

### 1. A segment's last page is free

`pagination::place_segment` no longer charges page badness on the final page of
any segment. A segment boundary already means "the next page starts here":
a chapter opening (`page-break-before`), a forced break, or the end of the
document. Filling every earlier page and allowing only that final page to be
short is the book-correct greedy tail; it also removes the two half-empty
leaves without changing the page cost model, the kernel, or the hash schema.

### 2. Running heads name a chapter, never a section

The running head uses the last **level-1** heading whose flow item lies on the
page. In `book` mode a chapter opens its own page, so this equals the chapter
at page start. In `sample` mode a chapter may open mid-page; the page then
carries the chapter it introduces, which is what a reader sees as "the current
chapter". A level-2+ heading is never promoted into the header.

Running heads and folios now resolve generated styles `runningHead` and
`folio`. They inherit the document style when a template does not mention
them, so older templates keep their centred look; `song.yaml` sets sans 8.5 pt,
normal weight, 60% ink, 0.05 em tracking and left alignment for the head, and
sans 9 pt, 60% ink, centred for the folio.

### 3. `layout.mode` is a template property

The template gains an optional top-level `layout: { mode: book | sample }`.
`book` is the default and is omitted from canonical JSON, so every existing
template keeps its bytes. `sample` suppresses the page break of a level-1
heading only; no other block's `page-break-before` changes. The full-capability
build selects `sample` in the template, not with `sed`.

### 4. `book-v1` preset

`publish-v1` keeps its 2 em short-last threshold. A new `book-v1` preset is
`publish-v1` with `short_last_threshold = 3em`; the book sample selects it via
`examples/book/book.engine`. The command-line default remains `latex-v1`, so a
LaTeX-compatible build still leaves the last line ragged. The short-last charge
remains a soft cost; it never forces a line break that the kernel would not
otherwise choose.

### 5. Generated book styles

The template gains generated element names without new YAML keys:

- `tocChapter`, `tocSection`, `tocListTitle`: independent contents rows and the
  figure/table list heading. They inherit `toc`/`tocTitle` when undefined, so
  templates that predate them are unchanged.
- `coverColophon`: a fifth cover line read from `cover.fields["colophon"]`.
  LaTeX writes that field with `\colophon{...}`.
- `runningHead`, `folio` as above.

### 6. Figure width and alignment

The template gains `figure: { width: natural | half | full, align: left |
center | right }`, defaulting to `full`/`center` and omitted from canonical JSON
when default. `natural` uses the image's intrinsic pixel size at 96 dpi, capped
at the measure; the layout never invents a physical width. DocIR still carries
only the path and intrinsic size, which closes D110 at the template seam.

## Consequences

- A chapter can end with one short page, but never with two balanced half
  pages. The corpus page counts and the `ALL` gate line are unchanged.
- A running head is stable within a chapter and never shows a section title.
- `full.sh` no longer edits style declarations; the sample template states its
  flow.
- New template keys are additive. Canonical template JSON writes them only when
  non-default, so historical template hashes are preserved. New generated
  styles inherit their base style, so templates without the new element names
  keep their appearance.
- `soma-latex` accepts `\language{...}` (metadata/PDF `/Lang`) and
  `\colophon{...}` (cover field). Both are preamble metadata, not body content.
- Not included: two-column indexes for entries above 24, a rule under the
  running head, double-sided binding margins, a per-character fallback report,
  and the D109 mixed-syntax footnote semantics. These remain in
  `docs/debt.md` (D109, D111, D112); the current sample index is short and the
  head rule is cosmetic.
