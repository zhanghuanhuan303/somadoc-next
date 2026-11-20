# ADR-0046: a CFF subset is compacted, and the charset carries the CIDs

- Status: Accepted
- Date: 2026-11-20
- Supersedes: ADR-0039 §3 ("subset by blanking, not by renumbering") and the
  second-to-last consequence of ADR-0039
- Related: ADR-0034 (PDF backend), ADR-0040 (name-keyed CFF), ADR-0045
  (deflate), `docs/debt.md` D82
- Touches: `soma-render-pdf` only. No schema, no layout hash.

## Context

ADR-0039 embedded a CID-keyed CFF by **blanking** unused glyphs: the
CharStrings INDEX kept every slot, an unused glyph became the single byte
`14` (`endchar`), and the charset was rewritten to identity. It kept the id
space trivial — glyph id, CID, the code in the content stream, the keys of `/W`
and `/ToUnicode` were all the same number — at the cost of a 65,535-entry INDEX
whose offset table alone is ~260 KB on Noto CJK, plus one byte per unused
glyph.

That was the right first move (correctness first), but the cost is pure waste:
a 49-page anthology uses a few thousand glyphs out of 44,810, and an eight-page
sample uses a few hundred.

## Decision

### 1. The CharStrings INDEX holds only the glyphs that are used

`subset` builds

```text
order = [0] ++ (used glyph ids, ascending, deduplicated)
```

and writes one CharStrings entry per element, in that order. Unused glyphs do
not become blank programs; they do not exist. `order[0]` is always glyph 0,
because index 0 is `.notdef` in every CFF reader.

### 2. The charset is the bridge: new glyph id → original id as the CID

The content stream writes the **original** glyph id as the two-byte code, and
`/W` and `/ToUnicode` are keyed by that code, so the CID must not change. A
CID-keyed CFF resolves a CID through its charset, so the charset is written as

```text
format 0: CID for glyph 1 … CID for glyph n-1, where CID_k = order[k]
```

A few hundred entries of two bytes each replace the 65,535-entry INDEX. Glyph 0
is implicit, as always.

`CIDCount` becomes `max(CID) + 1`, not the glyph count. The number is larger
than the glyph count on a sparse CJK face, and that is fine: it bounds the CID
space, so `/W` and the content stream can keep addressing the CIDs they always
did.

### 3. FDSelect is rewritten into the new id space

`FDSelect` maps glyph id → Font DICT. Its ids changed, so it cannot be copied.
`fd_per_glyph` reads either format (0: one byte per glyph, 3: ranges) into a
per-glyph vector, the vector is gathered along `order`, and `write_fdselect`
emits format 3 with one range per run of equal indices. A converted name-keyed
face gets a single range, as before.

### 4. What does not move

The private-dict/subroutine region, the string index, the global subrs and the
Font DICT bodies are copied exactly as ADR-0039 described: charstrings call
local subrs by index, and a Private DICT's `Subrs` operand is relative to the
dict, so the region travels as one blob. Compaction does not touch it — that
blob is the next thing to attack, and it needs a different technique
(re-interpreting and inlining, which ADR-0040 already does for `seac`).

`seac` expansion is unchanged and still produced for name-keyed faces before
compaction, so a composite never points at a glyph that was dropped.

## Consequences

Measured on this machine, same layout, same fonts:

| sample | before (ADR-0039 + 0045) | compact (this ADR) | vs the stored-only 0034 baseline |
| --- | --- | --- | --- |
| `two-font.md`, Noto Serif CJK, 8 pages | 1,581,063 B | **1,247,396 B** (−21.1%) | 1,919,666 → 1,247,396 B (−35.0%) |

- Poppler resolves the embedded font: `pdffonts` still reports `CID Type 0C`,
  `pdftotext` extracts the text, and a rasterised page draws every glyph — the
  test that matters, because a wrong charset would silently swap outlines.
- The subset is now smaller than the source table, which the unit tests assert.
- The unit tests changed shape: `a_subset_compacts_the_glyph_slots_and_keeps_
  the_cids` checks the count, the moved program, the charset entries and
  `CIDCount`.
- ADR-0039's argument still stands for TrueType: `glyf`/`loca` subsets keep
  their id space, and `/CIDToGIDMap /Identity` still means "code = gid".

## Alternatives

- **Renumber the CIDs too** (compact CIDs, remap `/W` and `ToUnicode`). Fewer
  bytes in `/W`, but it would move the id space out of `PagedLayout`, where it
  is a glyph id, into the renderer — and the renderer would then own a mapping
  the canonical result does not know about. Rejected: the CID is the one number
  every layer already agrees on.
- **`/CIDToGIDMap`** only exists for `/CIDFontType2`. For `/CIDFontType0` the
  charset *is* the mapping, which is why this ADR rewrites it.
- **Drop the private-dict blob too.** Not this ADR: it needs the Type 2
  interpreter to inline every subroutine, and the interpreter currently only
  runs often enough for `seac`. Noto CJK's blob is ~1.2 MB, so it is the next
  real win.
