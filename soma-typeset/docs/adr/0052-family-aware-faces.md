# ADR-0052: a style picks its face by family, not by slot

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0012 (table-backed metrics), ADR-0034 (PDF backend), ADR-0047
  (image and tag seams), `examples/book/song.tex`
- Touches: `soma-metrics` (`family_is_sans`), `soma-layout` (face selection),
  `soma-render-pdf` (two new roles), `soma-cli` (two new flags).
  No schema change.

## Context

`docs/fonts.md` assumes a metrics table holds four faces: serif regular, sans
regular, serif bold, sans bold — the slots a style indexes into with

```rust
let base = if sans { 1 } else { 0 };
base + if bold { 2 } else { 0 }
```

Two problems with that.

1. **The layout indexed by position, the table was written in argument order.**
   `tools/metrics-gen` writes faces in the order the `--face` flags appear. A
   table generated as `serif:normal, serif:bold, math, fallback` therefore
   answered a *sans* request with the serif bold face and a *bold* request with
   the math face. Nothing noticed because the shipped templates only ever asked
   for serif regular.
2. **The PDF backend had no sans role at all.** `role_for` knew Regular, Bold,
   Math and Fallback, so even with a correct table a style that asked for a
   sans family would be drawn with the serif face. "宋体正文 + 黑体标题", the
   default Chinese book arrangement, was impossible in one document.

## Decision

### 1. Family matching is one function, in the metrics crate

```rust
pub fn family_is_sans(family: Option<&str>) -> bool
```

`sans`, `mono`, `hei`, `gothic` select the sans family; everything else,
including an empty value, selects the serif family. Matching is ASCII
case-insensitive and allocation-free. It lives next to the face constants
because both the layout and the renderer have to agree on it, and they must not
each keep their own copy of the rule (they did, until now).

### 2. `face_of` searches by family and weight

```rust
faces.iter().find(|face| face.family == family && (face.weight != 0) == bold)
     .or_else(|| faces.iter().find(|face| face.family == family))
     .or_else(|| table.face(0))
```

Position in the blob no longer matters, a table may hold any subset of
families, and a missing bold face falls back to the family's regular face
before it falls back to the first face at all.

### 3. The backend gains `Sans` and `SansBold`

`Role` becomes six values (`F1`…`F6`), `PdfFonts` gains `sans` and
`sans_bold`, and `role_for` resolves in the same order the layout measures in:
fallback → family (sans?) → weight. The CLI gains `--font-sans` and
`--font-sans-bold`, and `scripts/song.sh` passes all six faces for the sample
that exercises them.

## Consequences

The sample `examples/book/song.tex` (思源宋体正文, MiSans 标题, 楷体回退,
Noto Sans Math) embeds five distinct faces in one PDF:

| role | face | stream |
| --- | --- | --- |
| `SomaSerif` | Source Han Serif CN Regular | `/FontFile3` (CID Type 0C) |
| `SomaSerif-Bold` | Source Han Serif CN Bold | `/FontFile3` |
| `SomaSans` | MiSans Normal | `/FontFile2` |
| `SomaSans-Bold` | MiSans Bold | `/FontFile2` |
| `SomaMath` | Noto Sans Math | `/FontFile2` |
| `SomaSerif-Fallback` | Fandol Kai | `/FontFile3` |

- A template switches family with `family: sans` and weight with `weight: bold`
  — the property names the template language already had.
- The family rule is now tested (`family_is_sans` covers `Heiti SC`,
  `Source Han Gothic`, `monospace` and the negative cases), so the two layers
  cannot drift apart silently again.
- A metrics table generated in an unusual order now behaves correctly. The
  embedded tables are unchanged.

## Alternatives

- **Sort the face vector when writing the table.** Fixes the symptom for tables
  we generate, leaves any hand-written or future table at the mercy of the
  order, and does nothing for the renderer's missing roles.
- **Add a `family` byte to `PlacedBlock`.** The renderer already receives
  `ComputedStyle` per atom; the information was there, only the lookup was
  wrong.
- **Treat "bold" as "the sans face"** (the trick the sample used before this
  ADR). It works for headings but turns every inline `\textbf` into 黑体, which
  is a different decision than the author made.
