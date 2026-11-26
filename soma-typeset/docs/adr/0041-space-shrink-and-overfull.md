# ADR-0041: a space gives up a third of itself; an overfull line is not a break

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0036 (line-end trim), ADR-0007 (frozen inline kernel),
  ADR-0014 (typography refinements), `docs/kp-model.md`, `docs/params.md`
- Touches: `soma-core` (`model_version` 2), `soma-layout` (space glue),
  `soma.layout/6` canonical values

## Context

D86 recorded what an all-Latin paragraph looks like when the model is asked to
fit a line it cannot fit. Measured on SF Pro Display (name-keyed CFF, 1000-unit
sample at 10.5 pt, 29.3 em measure):

| | value |
| --- | --- |
| natural width | 126,188 (1/4096 em) |
| target | 120,079 |
| shrink available | 5,633 (11 spaces × 1/8 em) |
| ratio | 1084 ‰ (`overfull: true`) |
| result | every space crushed from 0.29 em to 0.15 em — words touch |

Two things were wrong, and they are independent:

1. **A space's shrink was a fixed 1/8 em.** How much a space may give up is a
   property of the space, not of the em: 1/8 em is 43% of a 0.29 em space and
   26% of a 0.38 em one. TeX's space glue shrinks by a third of *its own*
   width, which is font-relative by construction.
2. **An overfull justified line was a normal break.** `eval_line` computed
   `overfull = excess > shrink` but did nothing with it: no extra cost, no
   rejection, and `adjustment = -excess` still pulled the line onto the
   measure — by squeezing glue past the shrink it declared. TeX calls this an
   overfull box and gives it infinite badness.

The two together mean the breaker can pick a line it cannot justify and then
justify it anyway, visually.

## Decision

### 1. A space shrinks by at most a third of its advance

The clamp lives in `soma-layout`, not in `soma-rules`: the rule table is
font-blind (it produces `SPACE_SHRINK` without knowing the face), and the space
advance only exists once metrics are resolved. `break_paragraph` now clamps
every `BoundaryKind::Space` boundary to

```text
glue.shrink = min(glue.shrink, metrics.space_width(style) / 3)
```

before it hands the paragraph to the kernel. CJK glue is untouched — its
`1/32 em` gaps have no relation to a space advance, and a Chinese line
compresses by having many of them.

### 2. An overfull justified line is reachable only through the fallback ladder

In `eval_line`, `overfull = shrink <= 0 || excess > shrink` now takes the
emergency path: the line is rejected in the normal stage and, when the ladder
allows it, costs `costs.emergency` (10,000,000 in `default-v1`) and is marked
`emergency`. This is TeX's "badness 10000": the DP will choose any legal break
first, and a line that truly cannot be justified is reported instead of being
silently crushed.

`model_version` becomes 2. The weights of `default-v1` are unchanged; what
changed is which breaks the model considers feasible, so `totalCost` from
model 1 and model 2 must not be compared.

## Consequences

- The corpus in Gate Q′ goes from **3 over-compressed lines to 0**
  (`classics` 2 → 0, `mixed` 1 → 0) with the same page and line counts.
  `ratio_jitter_p95` rises from 351 to 368 as those lines' slack is
  redistributed; `bdev_rel_error_p95` is unchanged at 9.
- The SF Pro sample now breaks before the offending word: line 1 ends at
  "durable", the spaces stay near 0.29 em, and text extraction sees words
  again instead of one 64-character run.
- Every canonical hash moves (a new `model_version` is in `paramHash`). The
  SVG golden moved by at most 0.006 em per atom with identical line breaks —
  the example book's Chinese text is barely affected because it has few
  spaces.
- A paragraph whose measure genuinely cannot hold a line (one long word) still
  renders: the ladder reaches the emergency stage and the line is flagged.
  That is the same trade ADR-0036 makes for over-long words.
- `soma-demo`'s "the two algorithms differ" fixture needed a 10 em measure:
  at 14 em both algorithms now refuse the same overfull lines and agree.

## Rejected

- **Lowering `SPACE_SHRINK` in the rule table.** It would still be an em
  constant, so it would be wrong for the next face; and it does nothing about
  the silent overfull.
- **Raising `badness_scale` for compression.** The cost is already cubic, but
  a *feasible* break should stay cheap: the problem was feasibility, not price.
  Making a 1084‰ compression merely expensive would still render it when no
  alternative exists, and would not explain why.
