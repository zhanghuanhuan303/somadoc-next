# Choosing a body font

The engine never opens a font: layout reads a `soma.metrics/7` table, the PDF
backend embeds `glyf` outlines or CID-keyed CFF programs (ADR-0039). A face is
therefore usable for PDF only when it has one of those two outline formats. `tools/metrics-gen --audit` scores every face on disk
against what Chinese body typesetting actually needs, so the choice is a
measurement instead of a guess.

## The audit

```sh
cargo run -p tools/metrics-gen -- \
  --audit /usr/share/fonts \
  --audit ~/.fonts \
  --top 40
```

Output is CSV on stdout, one row per face:

```text
score,verdict,path,face,glyphs,coverage,uniform,punct,ink_range,ink_mean,ink_std,han_density,punct_density,line,outlines
```

TTC collections are expanded face by face. `--top` only limits the rows printed;
the verdict count on stderr covers every face.

### Hard gates

A face is rejected before scoring when any of these fails:

| Gate | Why |
| --- | --- |
| `glyf` + `loca` present | the audit reads `glyf` to measure marks; a CFF face (CID-keyed or name-keyed, ADR-0039/0040) is embeddable but its ink is unknown here, so it is reported as `no-outlines` |
| CJK coverage ≥ 95% | missing Han characters fall back mid-paragraph |
| Han advance is 1em for ≥ 95% of the sample | a proportional Han face breaks justify and the baseline grid (`proportional-han`) |
| Full-width marks are 1em wide for ≥ 90% | the line-width accounting assumes it (`bad-punctuation`) |

`no-cjk` means the face has no Han coverage at all (icon fonts, Latin faces).

### Score (0–100)

Only faces that pass the gates are ranked.

| Weight | Metric | Definition |
| --- | --- | --- |
| 30 | `coverage` | hit rate over 512 evenly spaced probes in U+4E00–U+9FFF, plus 13 full-width marks and the 100 most frequent Han characters |
| 15 | `uniform` | share of 256 sampled Han glyphs whose advance is exactly 1em |
| 30 | `ink_consistency` | `1 - clamp((std(ink_right) - 0.10) / 0.25, 0, 1)` over the 13 marks |
| 15 | `punct` | share of the 13 marks drawn one em wide |
| 10 | `line` | `1 - clamp(|(ascent+descent)/1em - 1.15| / 0.35, 0, 1)` |

`han_density` and `punct_density` are the mean `ink / advance` over sampled Han
glyphs and the 13 marks. They do not score either — they are the input to the
font-profile proposal in [`docs/font-preset.md`](font-preset.md), and they show
how much of its em box a face actually fills.

Within one face the advance-versus-ink error is not a constant: lines differ in composition, and the example book's justified lines range from 0.753 to 0.841 ink/advance. [`docs/font-preset.md`](font-preset.md) turns that into a unified width model.

`ink_mean` and `ink_range` are reported but do **not** score: how much of the
advance a mark's ink fills is a weight and style decision, not a quality
signal. Consistency is the signal — a face whose comma and full stop have very
different bearings puts a tight line end next to a loose one.

Ranking ties are broken by path and face index, so a rerun against the same font
directory prints the same table. Scores within **2 points** of each other are
noise; treat them as one band.

## Measured on the development machine (2026-11-20)

2,982 faces in 2,881 files: **31 usable**, 24 low-coverage, 367 without
outlines, 2,560 without CJK.

| Score | Face | Glyphs | ink_mean | ink_std | han_density |
| --- | --- | --- | --- | --- | --- |
| 84.3 | WenQuanYi Micro Hei (`wqy-microhei.ttc`) | 34,600 | 0.472 | **0.197** | 0.892 |
| 81.7 | SimHei (`Simhei.ttf`) | 28,522 | 0.497 | 0.217 | 0.908 |
| 81.1 | SimSun (`SIMSUN.ttf`) | 22,075 | 0.517 | 0.222 | 0.898 |
| 81.0 | MiSans Regular | 29,414 | 0.514 | 0.222 | 0.864 |
| 75.5 | LXGW WenKai Regular | 46,490 | 0.529 | 0.268 | 0.906 |
| 75.5 | LXGW WenKai GB Regular | 36,258 | 0.529 | 0.268 | 0.901 |
| 74.2 | IBM Plex Sans SC Regular | 29,285 | 0.544 | 0.279 | 0.905 |

The spread comes almost entirely from `ink_std`. MiSans and the Song/Hei faces
keep their marks within a narrow band; Kai-style and Latin-first CJK faces
spread wider, which shows up as uneven line ends even when every break is
optimal.

Noto Sans/Serif CJK (`.ttc`, 65,535 glyphs) is the one surprising `no-outlines`
row: it has the best coverage here, but the distributed collection is
CFF-outlined, so **this audit** cannot measure its marks. That is a limit of the
audit, not of the generator: `metrics-gen` runs a bbox-only Type 2 interpreter
over CFF charstrings, so a table generated from these faces does carry
punctuation ink (ADR-0073). The committed `noto-cjk.smt` is regenerated at `soma.metrics/7`, so it carries
that ink too and the *default* path (no `--metrics`) trims against the real
glyphs (ADR-0074).

## Automatic selection

```sh
scripts/fonts.sh                    # -> examples/book/metrics-auto.smt
scripts/fonts.sh /tmp/my-table.smt
```

The script audits the usual directories, then picks the highest-scoring face
whose family is in a redistributable allow-list (OFL first, GPL-with-exception
second, anything usable last). Inside a family it requires a `Regular` or
`Normal` cut: a Light or Thin face would quietly set the whole document
lighter. A `Bold` companion is added when the file naming offers one, and the
Noto CJK face when present. The math face is added by `scripts/math-face.sh`,
which prefers New Computer Modern Math and falls back to Latin Modern/DejaVu/Noto.

`--font-math` names the face the PDF draws formulas with. When it is omitted
and the document has a formula, `soma render --format pdf` uses the bundled
New Computer Modern Math first, then probes installed faces and accepts only one
whose advances agree with the metrics table (ADR-0077). The bundle is the face
the embedded table was measured from, so the default works on a machine with no
TeX installation; a custom table measured from another face falls through to
Latin Modern, Noto Sans Math, DejaVu Math TeX Gyre, TeX Gyre Pagella Math,
Libertinus Math, STIX Two Math or Cambria Math. A different math face resolves the same variant
ordinals but draws them at other sizes, so the probe compares advances rather
than names. `--stats` reports which face was chosen; a document whose formula
symbols fall back to the body face prints a warning naming the flag.

`scripts/book.sh` falls back to the committed `metrics-lxgw.smt` when no
TrueType font exists, so HTML, SVG and JSON still reproduce the example book's
line breaks. Only the PDF step is skipped.

## Fingerprints and recognition

The same measurements double as a fingerprint, so a face you have never seen
can be placed next to the ones you have.

```sh
# Build a library once
cargo run -p tools/metrics-gen --   --audit /usr/share/fonts --audit ~/.fonts   --fingerprints /tmp/fonts.csv --top 1

# Ask about a face that is not in the library
cargo run -p tools/metrics-gen --   --identify serif:normal:/path/Unknown.ttf --library /tmp/fonts.csv
```

Nine axes, all normalised to 0–1:

| Axis | Meaning |
| --- | --- |
| `coverage` | CJK coverage (same probe set as the score) |
| `uniform` | share of Han glyphs exactly 1em wide |
| `punct_em` | mean full-width mark advance / 1.2em |
| `ink_mean` | mean gap between mark ink and its advance |
| `ink_std` | spread of those gaps / 0.5 |
| `ink_asym` | `(ink_left_mean - ink_right_mean + 1) / 2` |
| `latin_spread` | `(advance('W') - advance('i')) / 1em` |
| `digit_uniform` | share of digits sharing one advance |
| `line` | `((ascent+descent)/1em - 0.8) / 0.6` |

Distance is a weighted Euclidean (`[3,2,1,1,2,1,1,1,0.5]`, averaged). The three
ink axes are **skipped when either side has no outlines**, so a CFF face is
compared on what is actually known about it instead of looking like a face with
zero ink. Results are collapsed to one row per family: eight weights of one cut
are one answer.

`--identify` also prints a profile derived only from measurements:

```text
profile: cjk/monospaced/full-width/w400
distance,path,face,profile
0.0048,/usr/share/fonts/TTF/LXGWWenKai-Light.ttf,0,cjk/monospaced/full-width/w300
0.0244,/home/badleric/.fonts/IBMPlexSansSC_Regular.ttf,0,cjk/monospaced/full-width/w400
```

`script` is `cjk` / `cjk-partial` / `latin`, `width` is `monospaced` /
`proportional`, `punct` is `full-width` / `mixed` / `half-width`, and the last
token is the `OS/2` weight class. Style — Song, Hei, Kai, Yuan — is **not**
guessed: it is a design decision that the outline bounding box cannot recover
reliably, so the fingerprint stops at what it can measure.

## Coverage and fallback

The fingerprint says what a face *is*; it says nothing about what it *lacks*.
Two more commands close that gap — see [`docs/mixed-font.md`](mixed-font.md):

```sh
metrics-gen --coverage serif:normal:/path/Body.ttf --text book.tex
metrics-gen --fallback serif:normal:/path/Body.ttf --text book.tex --audit /usr/share/fonts
```

`--coverage` reports how much of a document's character set the face can draw
and lists what is missing. `--fallback` ranks installed faces by how many of
those characters they rescue, preferring a matching weight before a matching
style, and skips faces with neither `glyf` nor a CID-keyed CFF program, because
a rescue face has to be embeddable.

## Two typefaces in one line

A Latin face can be told to record **only non-CJK codepoints**:

```sh
cargo run -p tools/metrics-gen -- \
  --out split.smt \
  --face serif:normal:/path/IBMPlexSansSC-Regular.ttf:latin \
  --face fallback:normal:/path/MiSans-Normal.ttf \
  --face math:normal:/path/NotoSansMath-Regular.ttf
```

The fourth segment of `--face` is `latin`. Such a face measures Latin and
digits; a Han glyph is absent, so the rescue machinery from
[`docs/mixed-font.md`](mixed-font.md) draws it with the fallback face. One
line, two typefaces, and the breaker still measures every atom with the face
that draws it.

Render it with the same two files:

```sh
soma render --input book.md --template book.yaml --metrics split.smt \
  --format pdf --font IBMPlexSansSC-Regular.ttf \
  --font-fallback MiSans-Normal.ttf --font-math NotoSansMath-Regular.ttf
```

`examples/book/out/two-font.pdf` is an eight-page sample built this way.

## Producing a table for the chosen face

```sh
cargo run -p tools/metrics-gen -- \
  --out examples/book/metrics-lxgw.smt \
  --face serif:normal:/usr/share/fonts/TTF/LXGWWenKai-Regular.ttf \
  --face serif:bold:/usr/share/fonts/TTF/LXGWWenKai-Medium.ttf \
  --face math:normal:/usr/share/texmf-dist/fonts/opentype/public/lm-math/latinmodern-math.otf
```

`examples/book/metrics-lxgw.smt` is committed as a freely redistributable
layout baseline (391 KB, `soma.metrics/7`, per-glyph bearings for 46,473 glyphs):
pointing `--metrics` at it reproduces the same line breaks on any machine
without the font installed. Rendering a PDF still needs the font file itself,
because the PDF embeds outlines.

## Licences

Metrics are measurements, not font programs, but ship only tables built from
faces whose licence clearly allows redistribution:

| Face | Licence |
| --- | --- |
| Noto CJK | SIL OFL 1.1 |
| New Computer Modern Math | GUST Font License |
| Latin Modern Math | GUST Font License |
| LXGW WenKai | SIL OFL 1.1 |
| WenQuanYi Micro Hei | GPL v3 + font embedding exception |
| MiSans | free to use; redistribution of the font is restricted — its table is generated locally, not committed |

New Computer Modern Math is also bundled as
`crates/soma-cli/assets/NewCMMath-Regular.otf`, with the GUST license text next
to it; it is the only font binary in the repository. All other listed faces
contribute measurements only.

`PROVENANCE.md` records the source file hash of every committed table.
