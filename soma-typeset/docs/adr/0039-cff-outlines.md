# ADR-0039: CFF outlines in the PDF backend

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0034 (PDF backend), ADR-0035 (metrics from a font file),
  ADR-0037 (punctuation ink), ADR-0006 (determinism), `docs/fonts.md`
- Touches: `soma-render-pdf` only. No schema changes.

## Context

The PDF backend embeds a font by rebuilding its `glyf` outlines and writing a
`/FontFile2` program (ADR-0034). `Face::open` rejected `OTTO` with "PDF output
needs a TrueType (glyf) font; CFF/OTF is not supported".

That rejection excludes the best-covered Chinese face on this machine: Noto
Sans/Serif CJK ships as a `.ttc` whose outlines are CFF (Type 2 charstrings),
44,810 glyphs per face. `docs/fonts.md` records it as the one surprising
`no-outlines` verdict, and the two-font samples had to use TrueType faces for
Han (MiSans, LXGW WenKai) even when a Song or Hei style was wanted.

CFF is a different container, not a different layout problem. The breaker never
sees outlines; the PDF backend needs three things from a face: a cmap, advance
widths, and a font program whose glyph ids match the ids the text operator
writes. Everything else (line metrics, `/W`, `ToUnicode`) is already derived
from `head`/`hhea`/`hmtx`/`cmap`, which OTF has.

## Decision

### 1. Accept `CFF `, reject `CFF2`

`Face::open` now resolves outlines to either `glyf`+`loca` or a `CFF ` table.
`CFF2` is refused with a clear message: CFF2 carries `blend` operators whose
default-instance evaluation is a different job, and no Japanese/Chinese body
face ships as CFF2 today.

### 2. Embed as `/CIDFontType0` + `/FontFile3 /Subtype /CIDFontType0C`

| | TrueType | CFF |
| --- | --- | --- |
| stream | `/FontFile2` | `/FontFile3` with `/Subtype /CIDFontType0C` |
| descendant | `/CIDFontType2` | `/CIDFontType0` |
| CID → glyph | `/CIDToGIDMap /Identity` | the charset inside the embedded CFF |

`/W` still comes from `hmtx`, `/DW` stays 1000, and `Identity-H` still means
"the two-byte code in the content stream is the CID".

### 3. Subset by blanking, not by renumbering

The TrueType path keeps every glyph id and empties unused `loca` ranges, so the
shaper, the text writer, `/W` and `ToUnicode` all share one id space. CFF does
the same:

- The CharStrings INDEX is rebuilt with **the same glyph count**; a used glyph
  keeps its Type 2 program verbatim, an unused glyph becomes the single byte
  `14` (`endchar`, a valid empty glyph).
- The charset is rewritten to **identity** (`CID n` ↔ `GID n`). Noto CJK's
  original charset maps glyph ids to Adobe-Japan1/GB CIDs; rewriting it makes
  the embedded font's CID space equal to the id space the rest of the pipeline
  already uses, and it is what a name-keyed CFF needs anyway to become
  CID-keyed.
- `CIDCount` is set to the glyph count.
- `FDSelect` maps glyphs to Font DICTs and is copied unchanged (ids are
  unchanged).

The price is a 65,535-entry INDEX offset table plus one byte per unused glyph —
~260 KB on Noto Serif CJK against a ~1.2 MB private-dict/subroutine blob. That
is cheaper than teaching the renderer a second id space, and it keeps the
determinism story unchanged: the same layout produces the same codes.

### 4. FDArray, private dicts and local subrs move as one blob

Charstrings call local subrs by index, and a Private DICT's `Subrs` operand is
relative to the start of that dict, so the region from `FDArray` to the end of
the CFF table is copied verbatim. Only the absolute `Private` offsets inside
each Font DICT are re-encoded — as `29` (32-bit) operands, which are a fixed
five bytes, so the rebuilt INDEX has a predictable size and the region's new
start can be computed before writing it.

### 5. A name-keyed CFF is refused, not converted

`FDArray`/`FDSelect` are mandatory in the PDF CFF flavour, so a name-keyed
program (most Latin OTFs) would have to be promoted: a synthesised
`Adobe-Identity-0` `ROS`, a Font DICT holding the original `Private` dict, an
`FDSelect`, and — the part that matters — an **identity charset**, because a
CID-keyed font's CID is resolved through that charset and this backend's codes
are glyph ids.

Replacing the charset breaks accent composition. Type 2 `endchar` with four
operands (`seac`) names its base and accent glyphs by *StandardEncoding code*,
and the name is resolved through the charset — the table just replaced. The
glyph would render as something else, silently.

Refusing is the honest option, and it costs little here: every CJK OTF that
matters (Noto CJK, Source Han) is CID-keyed, and Latin faces are available as
TrueType. The error names the limitation and the way out. Supporting
name-keyed faces later means a Type 2 interpreter that follows subroutines and
keeps their argument stack — a `callsubr` leaves operands behind, so a scanner
without subr semantics cannot even tell whether `endchar` had four operands
(the first attempt at this scanner mis-read glyph 18817 of Noto Serif CJK for
exactly that reason).

### 6. `CIDSystemInfo` follows the embedded font's `ROS`

The PDF `/CIDSystemInfo` is written from the CFF `ROS` (registry, ordering,
supplement) rather than hard-coded `Adobe-Identity`. For Noto CJK that is
already `Adobe-Identity-0`.

## Consequences

- Noto Sans/Serif CJK can now be a body or fallback face in PDF output. Nothing
  in `soma-core`, `soma-layout`, the schemas or the canonical bytes changes.
  A CID-keyed face and a TrueType face can sit in the same document: the
  two-font sample renders Latin from IBM Plex (TrueType) and Han from Noto
  Serif CJK (`CID Type 0C`).
- A name-keyed CFF face fails at `PdfFont::subset` with a message that names
  the reason and the alternatives; it does not fail at PDF render time with a
  generic error. This is D85.
- A metrics table for a CFF face records every covered codepoint instead of
  collapsing uniform ranges into blocks. `Face::has` treats the per-glyph ink
  list as authoritative and blocks as a hint, and that list needs `glyf`; a
  block would therefore claim characters the face does not have. Noto CJK has
  no U+2A6A5, but a sparse extension-B block claimed it, the rescue face was
  skipped, and the glyph reached the page as `.notdef`. The table is ~2x larger
  for a CJK CFF face; the alternative is a schema version.
- `metrics-gen --audit` keeps its `glyf` requirement for **ink**: `ink_bounds`
  reads `glyf`. A CFF face still reports `no-outlines` there, but that verdict
  now means "this tool cannot measure its marks", not "the PDF backend cannot
  embed it". `docs/fonts.md` says so.
- The subset cannot drop unused glyph *slots*, so a CJK subset keeps a
  65,535-entry CharStrings INDEX (~260 KB on Noto) plus the private-dict blob
  (~1.2 MB). The eight-page sample with two faces is 1.9 MB. Accepted:
  correctness of the id space first, compression (no Flate in the writer yet)
  later.
