# Proposal: a typographic profile per font

**Status**: proposal, not implemented. Touches no frozen interface yet.
**Related**: ADR-0006 (determinism), ADR-0036/0037 (line-end bearing, ink),
`docs/fonts.md` (audit and fingerprint), `docs/mixed-font.md` (fallback).

## 1. The problem in one number

The engine charges a line for the **advance** of every atom. A font's advance
is not how much ink it puts on the page:

| Face | Han ink / em | Mark ink / em |
| --- | --- | --- |
| MiSans Thin | 0.839 | 0.173 |
| MiSans Normal | 0.861 | 0.233 |
| MiSans Heavy | 0.904 | 0.343 |
| LXGW WenKai Light | 0.892 | 0.233 |

Measured on 256 sampled Han glyphs and 13 full-width marks (`--audit`, columns
`han_density`, `punct_density`).

A line of 30 Han characters is therefore **25.2 em of ink** in Thin and
**27.1 em in Heavy**, while the DP sees 30 em for both. Every geometric decision
downstream inherits that error:

- `badness` compares a deficit against a target that does not match the ink;
- `short_last` judges the last line by advance, not by what is visible;
- the justified gaps are computed to fill 30 em of advance, so a loose face
  ends up looking looser than the target and a heavy face tighter.

ADR-0037 fixed the largest single case (line-end marks, where the gap reaches
0.7 em). The rest of the alphabet still runs on a class constant.

## 2. Why not one Rust engine per font

The intuitive answer — "write an engine that recognises a font and customises
the line breaker for it" — turns into unmaintainable code the moment there are
more than three fonts:

- there is no finite set of fonts to write branches for;
- behaviour would live in code rather than in data, so it cannot be audited,
  diffed, or reproduced on another machine;
- two fonts with the same measurements would still get two code paths.

What *is* reproducible: **one measurement engine, one deterministic derivation,
one data record per font.** The font supplies numbers; the algorithm is shared;
the output is a small, printable profile. That is more "custom" than a hand
written branch because it adapts to every face, including one released tomorrow.

## 3. What a font may and may not decide

This is the load-bearing decision of the proposal.

| Parameter | Decided by | Why |
| --- | --- | --- |
| advance, ink box, x-height, cap-height | **font** (measured) | facts about the file |
| line-end trim | **font** (ADR-0037) | already implemented |
| visual line width (ink accounting) | **font** (derived) | see §1 |
| CJK/Latin gap | **font** (derived from x-height) | a design property of the pairing |
| line height | **font** (ascent/descent), template may override | measured |
| `line` penalty | **designer** | how strongly to discourage an extra line |
| `badness` exponent and scale | **designer** | how strongly to punish a loose line |
| `tolerance`, `short_last_*` | **designer** | how tolerant the book is |

A new font must not silently change the book's *style*, only its *geometry*. If
a font also moved `line` and `badness`, "switch the body face" would become
"switch the typographic taste", and no golden could survive it.

## 4. Three layers

### F1 — measurement (`soma.metrics/3`)

Extend the blob with what the derivation needs:

```text
per ink: u32 codepoint | i16 ink_left | i16 ink_right   (already, version 2)
new per face:
  i16 x_height | i16 cap_height
  u16 han_sample | u32 han_ink_units      (sum of ink widths over the sample)
  u16 han_sample | u32 han_advance_units
```

or, cheaper and equivalent for our purpose, keep the raw per-codepoint ink for
all glyphs in a coarser form: one byte per glyph expressing
`round(255 * ink_width / advance)`. Han density is a scalar; storing 20k of them
is 20 KB per face, and the derivation only needs the mean.

Either way the blob stays integers-only and hashable.

### F2 — derivation (`soma-font` or a `soma-engine` module)

```rust
pub struct FontProfile {
    pub visual_density: u32,   // permille: mean ink / advance over Han
    pub mark_density: u32,
    pub x_height: Width,
    pub line_height: Width,
    pub derived: bool,         // false when the table had no outlines
}

pub fn derive(metrics: &MetricsTable, face: usize) -> FontProfile;
```

Pure, integer, no floats, no allocation beyond the table walk. The result is
small enough to print and diff:

```json
{"face":0,"visualDensity":861,"markDensity":233,"xHeight":2100,
 "lineHeight":4096,"derived":true}
```

`derived: false` (CFF, synthetic metrics) means the line breaker keeps today's
class constants — a font without outlines simply has no profile.

### F3 — the breaker (`soma.layout/7` and a new engine preset)

The line's natural width becomes **ink-aware**:

```text
natural_visual(i,j) = sum(ink_width[k]) + sum(designed_gap[k])
```

where `designed_gap` is the boundary glue the rule table already defines for
CJK/Latin transitions, plus, for two Han characters, the font's own side
bearing: `advance - ink_width`, apportioned left and right.

The end-of-line trim stops being a special case: it is what falls out when the
last atom is charged by ink. `Line.natural_width` gains a companion
`visual_width` (or replaces it, with a schema change), and `ratio` is computed
from the visual deficit.

`AtomBox` keeps advance-based coordinates for rendering; only the *decision*
uses ink. This keeps renderers untouched at F3.

`BreakConfig` gains:

```text
font_profile: Option<FontProfile>   // folded into paramHash
```

so two runs are comparable only when they share a profile, exactly like
`metricsHash` today.

### F4 — validation

1. Same paragraph, ten faces: measure the coefficient of variation of line
   density. It must **fall**, not rise.
2. Breakpoints change rate against today's model, per face. A well behaved face
   should move few breaks; a very light face should move many, in the direction
   of more characters per line.
3. `K-P ≤ Greedy-C` must still hold; the brute-force oracle is unchanged
   because it uses the same cost model.
4. The example book must stay at 11 pages or explain why it moved.

## 5. Staging and risk

| Stage | Deliverable | Risk | Gate |
| --- | --- | --- | --- |
| F1 | metrics v3 (density, x-height) | low | old tables still parse |
| F2 | `FontProfile::derive` + report | low | derivation is deterministic; no DP change |
| F3 | ink-aware width in the breaker | **high** | every golden and hash moves; `soma.layout/7` |
| F4 | calibration and matrix report | medium | density variance down, pages stable |

F1+F2 can ship without touching a single breakpoint. F3 is the decision point:
it changes every line, so it needs the F4 evidence in hand.

## 6. What the fingerprint is for, then

The fingerprint ([`docs/fonts.md`](fonts.md)) answers "what is this face" —
useful before you have chosen one. The profile answers "how does this face
behave in the breaker" — useful after. They share the measurement layer and
nothing else. Neither replaces `--coverage`, which answers "can it draw this
document at all".

## 7. The first experiment I would run

Before F3, take three faces with the widest density spread on this machine
(MiSans Thin 0.839, MiSans Heavy 0.904, LXGW WenKai 0.892), set the same
paragraph, and measure:

- the visual ink density of each line;
- the spread of that density within a paragraph;
- how many lines end "short" in ink but "full" in advance.

If the within-paragraph spread is already above 5%, ink accounting is worth the
schema change. If it is below 2%, the current trim plus ADR-0037 has absorbed
most of the error and F3 can wait.
