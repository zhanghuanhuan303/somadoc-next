# ADR-0042: fitness demerits are not worth four times the DP

- Status: Accepted (rejects the K7 option)
- Date: 2026-11-20
- Related: ADR-0007 (frozen inline kernel), ADR-0041 (overfull lines),
  `docs/kp-model.md` §K7, `docs/corpus-calibration.md`
- Touches: `soma-eval` (a new reported metric). The breaker is unchanged.

## Context

`docs/kp-model.md` planned K7 as an optional `(breakpoint, fitness)` DP state
with TeX's adjacent-line demerit, gated on evidence:

> 相邻行 fitness / 行间节奏进入 DP 状态（MVP 不做）… 仅在评估证明需要时启动

ADR-0041 changed which lines are feasible, and the corpus's
`ratio_jitter_p95` moved from 351 to 368. That was the trigger to look.

## Measurement

`corpus/zh` plus the 30-page book fixture, `publish-v1`, 1,213 lines,
607 adjacent line pairs inside paragraphs. Fitness classes are TeX's: tight
(`r < −0.5`), decent (`−0.5 … 0.5`), loose (`0.5 … 1.0`), very loose (`> 1.0`).

| | count |
| --- | --- |
| adjacent pairs | 607 |
| pairs whose ratio differs by > 250 ‰ | 423 |
| … by > 500 ‰ | 21 |
| … by > 999 ‰ | 0 |
| lines in the tight / decent / loose / very-loose class | 0 / 1063 / 18 / 2 |
| **pairs whose classes differ by more than one** | **3** |

All three are in `classics`, and all three come from the same two very loose
lines:

```text
p0 b3  ratios [367, 351, 1024, 328, 0]
p0 b8  ratios [351, 314,  942,   0]     (4-line paragraph)
p1 b1  ratios [328, 302,  981, 337, 302, 0]
p1 b5  ratios [1024, 314, 314, 942, 0]
```

Those lines are **forced**. Re-running the same documents with
`badness_scale` raised from 100 to 300 and to 1000 — a tenfold steeper curve —
leaves `classics`'s break sequence unchanged (`[367, 351, 1024, 328, 0]`). There
is no legal alternative: every other break is overfull under ADR-0041 or
leaves an even worse neighbour. A fitness demerit would add cost to a choice
that has no competitor, so the line would stay exactly where it is.

## Decision

**K7 stays closed.** The cost of a fitness state is a fourfold DP table
(`tight / decent / loose / very loose` at every breakpoint), a new
`LayoutResult` field, a `modelVersion` bump, a brute-force reference that has to
enumerate the same demerits, and a weight to calibrate. The measured benefit
is zero lines: the only class jumps the corpus produces cannot be avoided by
any break.

Instead the property is **measured and reported**: `PageMetrics::fitness_jumps`
counts adjacent pairs whose classes differ by more than one, the corpus CSV
carries it as a column, and Gate Q′ prints it. If a future change makes loose
lines avoidable-but-chosen, the column will move and the decision can be
revisited with numbers.

Two things this does **not** say:

* It does not say the loose lines are good. `classics` p0 b3 has a line at
  +1024 ‰ next to lines at +350 ‰, which is visible as one airy line in a tight
  paragraph. It says a `(breakpoint, fitness)` DP would not remove it.
* It does not close the door on *rhythm* as a concept. If the corpus grows and
  the column reports avoidable jumps, the analysis above is one run away from
  being redone.

## Consequences

- `soma-eval` gains `fitness_class(ratio_milli)` and the `fitness_jumps`
  counter; the corpus CSV gains a column between `orphans` and
  `page_free_p95`. Nothing in the engine changes, so no hash moves.
- `docs/kp-model.md`'s K7 row records the rejection and the numbers behind it.
- The `ratio_jitter_p95` rise that started this analysis (351 → 368) is
  explained: it is the airing-out that ADR-0041's overfull rule causes in
  `classics`, not a new rhythm defect.
