# Choosing a body font

The engine never opens a font: layout reads a `soma.metrics/2` table, the PDF
backend embeds a TrueType face by `glyf`. A face is therefore usable only when
it can produce both. `tools/metrics-gen --audit` scores every face on disk
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
score,verdict,path,face,glyphs,coverage,uniform,punct,ink_range,ink_mean,ink_std,line,outlines
```

TTC collections are expanded face by face. `--top` only limits the rows printed;
the verdict count on stderr covers every face.

### Hard gates

A face is rejected before scoring when any of these fails:

| Gate | Why |
| --- | --- |
| `glyf` + `loca` present | the PDF backend subsets outlines; CFF faces are rejected there (`no-outlines`) |
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

| Score | Face | Glyphs | ink_mean | ink_std |
| --- | --- | --- | --- | --- |
| 84.3 | WenQuanYi Micro Hei (`wqy-microhei.ttc`) | 34,600 | 0.472 | **0.197** |
| 81.7 | SimHei (`Simhei.ttf`) | 28,522 | 0.497 | 0.217 |
| 81.0 | MiSans Regular | 29,414 | 0.514 | 0.222 |
| 81.0 | SimSun (`SIMSUN.ttf`) | 22,075 | 0.517 | 0.222 |
| 78.5 | IBM Plex Sans SC Regular | 29,285 | 0.544 | 0.279 |
| 75.5 | LXGW WenKai Regular | 46,490 | 0.529 | 0.268 |
| 75.5 | LXGW WenKai GB Regular | 36,258 | 0.529 | 0.268 |

The spread comes almost entirely from `ink_std`. MiSans and the Song/Hei faces
keep their marks within a narrow band; Kai-style and Latin-first CJK faces
spread wider, which shows up as uneven line ends even when every break is
optimal.

Noto Sans/Serif CJK (`.ttc`, 44,810 glyphs) is the one surprising rejection: it
has the best coverage here, but the distributed collection is CFF-outlined, so
the PDF backend cannot subset it. The committed `noto-cjk.smt` is built from
those faces and therefore has no ink section.

## Producing a table for the chosen face

```sh
cargo run -p tools/metrics-gen -- \
  --out examples/book/metrics-lxgw.smt \
  --face serif:normal:/usr/share/fonts/TTF/LXGWWenKai-Regular.ttf \
  --face serif:bold:/usr/share/fonts/TTF/LXGWWenKai-Medium.ttf \
  --face math:normal:/usr/share/fonts/noto/NotoSansMath-Regular.ttf
```

`examples/book/metrics-lxgw.smt` is committed as an **OFL-licensed layout
baseline**: pointing `--metrics` at it reproduces the same line breaks on any
machine without the font installed. Rendering a PDF still needs the font file
itself, because the PDF embeds outlines.

## Licences

Metrics are measurements, not font programs, but ship only tables built from
faces whose licence clearly allows redistribution:

| Face | Licence |
| --- | --- |
| Noto CJK / Noto Sans Math | SIL OFL 1.1 |
| LXGW WenKai | SIL OFL 1.1 |
| WenQuanYi Micro Hei | GPL v3 + font embedding exception |
| MiSans | free to use; redistribution of the font is restricted — its table is generated locally, not committed |

`PROVENANCE.md` records the source file hash of every committed table.
