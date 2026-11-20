# ADR-0064: K7 line rhythm as a preset

- Status: Accepted
- Date: 2026-11-21
- Closes: D12
- Related: ADR-0007 (frozen kernel), ADR-0016 (engine presets),
  `docs/params.md` (`features.fitness`, `costs.fitness`), `docs/debt.md` D12
- Touches: `soma-core` (breaker, `ParamSet::rhythm_v1`), `soma-engine`
  (`preset: rhythm-v1`), docs

## Context

`features.fitness` and `costs.fitness` were reserved in the parameter set from
K1 onward, documented as "K7 only", and hashed — but the breaker never read
them. The result was a model where two adjacent lines could be one very tight
and the next very loose, paying only the absolute badness of each line. Readers
notice that alternation as a "striped" paragraph even when every line is
individually acceptable.

## Decision

### 1. Looseness classes, not raw ratios

A justified line falls into one of four classes by `ratio_milli`:

| class | ratio |
|---|---|
| tight | `< -500` |
| normal | `-500 … 500` |
| loose | `500 … 1500` |
| very loose | `> 1500` |

The classes are boundaries of the existing ratio, not new measurements.
Ragged lines (paragraph end, forced segment end) sit at ratio 0 and reset the
chain: a paragraph does not compare its last line with the next paragraph.

### 2. The class of the previous line is DP state

`optimal_with_fitness` runs the same DP as `optimal`, with four cells per break
point instead of one. A candidate that moves the class by `d` pays
`costs.fitness * d`; badness still prices the absolute looseness. State grows
4× and the candidate evaluation count grows 4× — the function is entered only
when both the feature and a nonzero weight are set.

Tie-break is unchanged: cost, then line count, then the smaller last-line
start; ties across classes fall to the smaller class.

### 3. Off by default, selectable as a preset

`default-v1` and `publish-v1` keep `fitness = false`, so their output is
byte-identical to before this ADR. `ParamSet::rhythm_v1()` is `publish-v1` with
`features.fitness = true` and `costs.fitness = 10`: one class step costs one
line. `soma.engine/1` accepts `preset: rhythm-v1`.

A zero weight with the feature on falls back to the old solver; a test pins
that the canonical bytes match `publish-v1`.

## Consequences

- The parameter hash already covered both fields, so no schema or hash change.
- The corpus baseline is measured with `publish-v1`; `rhythm-v1` is opt-in and
  reported separately if it is ever adopted as a default.
- The term is line-to-line inside a paragraph. Page-level grayness (how two
  paragraphs sit together) is still reported, not optimized: that would need a
  page-level objective and is a separate decision.
