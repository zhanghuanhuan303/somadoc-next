# Mixed fonts and missing glyphs

Two questions come up together: *what is this font I have never seen?* and
*what happens when one font cannot draw everything?* The first is answered by
the fingerprint in [`docs/fonts.md`](fonts.md). This document covers what the
engine can do today, what it cannot, and the plan for the gap.

## What works today

| Case | Supported | Mechanism |
| --- | --- | --- |
| Body serif and sans in one document | yes | `family` token selects face 0/1 (`serif`/`sans`), weight selects the bold pair |
| Bold and regular in one line | yes | `weight` selects face 2/3 |
| Inline and display math | yes | a dedicated `math` face, `AtomBox` is replaced by a math box |
| Chinese and Latin in one line | yes | one face, advances summed per character |
| A named family beyond serif/sans | no | four face slots plus math, chosen by substring match |
| A glyph the face lacks | **yes** | a `fallback:` face in the metrics table measures it; `TextSource::uses_fallback` tells the renderer which face drew it |
| Two faces in one line | **yes** | PDF embeds a second subset (`--font-fallback`); HTML/SVG let the browser pick |

## What a missing glyph costs

The kernel does not notice. `TableMetrics::advance` answers `default_advance`
for an unknown codepoint — one em for Han — so the line is measured as if the
character were there. Breakpoints, costs and pagination stay self-consistent.
Only the rendered output is wrong: the PDF draws a tofu box, HTML and SVG let
the browser pick a fallback, which is exactly the kind of non-reproducible
result the engine exists to avoid.

So a missing glyph is a **silent** failure. That is why the tools below check
coverage before layout rather than after.

## Knowing a font before you use it

```sh
# Does this face cover this document?
cargo run -p tools/metrics-gen -- \
  --coverage serif:normal:/path/Body.ttf --text book.tex

# Which installed face would rescue what is missing, and how close is it?
cargo run -p tools/metrics-gen -- \
  --fallback serif:normal:/path/Body.ttf --text book.tex \
  --audit /usr/share/fonts --audit ~/.fonts
```

`--coverage` counts every non-whitespace character in the file, reports the hit
rate, and prints up to 60 missing characters.

`--fallback` ranks installed faces by how many of those characters they rescue,
then by weight distance, then by fingerprint distance:

```text
primary: /usr/share/fonts/misan/MiSans-Normal.ttf#0
characters: 16, missing: 2
rescued,weight_gap,distance,path,face,profile
1/2,5,0.2585,/usr/share/fonts/TTF/LXGWWenKaiGB-Light.ttf,0,cjk/monospaced/full-width/w300
```

Weight distance comes before style distance on purpose: a Light face next to a
Regular body is visible on every rescued glyph, so a weight-matched stranger
beats a stylistically closer cousin. CFF faces are excluded — a rescue face has
to be embeddable, not merely measurable.

## Fingerprint and coverage answer different questions

- The **fingerprint** says what a face is: `cjk/monospaced/full-width/w400`,
  closest to these known faces.
- **Coverage** says what it is not: the characters it cannot draw.

A face can be a perfect stylistic match and still be unusable for a given
document, which is why both exist and why neither is inferred from the other.

## The architecture gap (closed for the drawable case)

Every atom currently carries `(x, advance)` and draws with its block's face
(`AtomBox` in `soma.layout/6`). Adding fallback needs three layers:

### L1 — measurement fallback (no schema change)

`Metrics` gains `glyph_face(text, style) -> Option<u16>`; `TableMetrics` and
`StyledMetrics` hold an ordered face chain and answer `advance` from the first
face that has the glyph. Widths, costs and breakpoints become correct for
documents with holes. Nothing in the canonical output changes, because the
breaker already only asks for widths.

This layer alone also fixes line-end trim for mixed lines: `scale_trim` asks the
same question to pick the right `ink_right`.

If the unified ink model in [`docs/font-preset.md`](font-preset.md) lands first,
L1 is what feeds it: the width model needs `face(k)` for every atom in order to
take that face's bearings. The model then handles mixed lines with no extra
rule — the gap between two faces is normalised to the script pair's design gap,
and a line ending in a fallback mark trims by that mark's own bearing.

### L2 — render fallback (needs `soma.layout/7`)

`AtomBox` gains `face: u16`, written during layout from the same chain. Then:

- PDF: `PdfFonts` grows from "role → one face" to "face index → font", and the
  subsetter runs per face. The PDF is already multi-font (body, bold, math); the
  change is bookkeeping, not new machinery.
- HTML/SVG: every atom is already its own positioned span, so each gets the
  family of its face.
- Canonical `soma.layout` and every golden move; the schema id must change.

### L3 — line box and marks

`line_vertical` already takes the maximum ascent/descent over the line's atoms;
it needs to read them from each atom's face. Marks then trim by the bearing of
the face that actually drew them (L1 provides the lookup).

## Staging

| Stage | Content | Gate |
| --- | --- | --- |
| P0 (this round) | `--coverage`, `--fallback`, this document | tools only |
| P1 | named faces + chains in `soma.metrics/3`; `Metrics::glyph_face` | layout golden unchanged |
| P2 | `AtomBox.face`, renderers, `soma.layout/7` | native/wasm hash; cross-renderer parity |
| P3 | per-face PDF subsets, one text run per face | PDF bytes, bookmarks |

P1 is the cheap half and P2 is the expensive half: the moment `AtomBox` grows a
field, every canonical hash and visual lock moves at once. P0 exists so the
decision can be made on data — how often does a real book actually need a
second face? The example book needs none: 580 characters, 100% covered by
MiSans.

## Cheaper alternatives that are not fallback

- **Pick a face that covers the text** (what `--audit` is for). This is the
  right answer for body copy; fallback is for the last 0.5%.
- **Pre-substitute characters** in the source. Rejected: it changes the
  document, and it hides the problem from the reader.
- **Let the browser fall back.** Rejected: HTML and PDF would disagree, and the
  whole point of the metric table is that they agree.
