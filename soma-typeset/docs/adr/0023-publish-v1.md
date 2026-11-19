# ADR-0023: Chinese publishing defaults (`publish-v1`)

- Status: Accepted (provisional until the blind test)
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0014 (typography refinements),
  `docs/params.md`, `docs/corpus-calibration.md`, Gate Q
- Changes: `soma-core` preset, `soma-rules` trim table, `soma-engine` default,
  `soma-eval` scorecard/corpus defaults, the blind kit's candidate.

## Context

`default-v1` was the frozen K-P reference: `tolerance = 3000` and a
conservative opening-punctuation trim of 1/4 em. The corpus sweep showed that
the normal path never needed `|r| > 1.5`, and the 2 over-compressed lines came
from the looser setting. Chinese publishing practice (W3C `clreq` §6.3 and
GB/T 15834-2011 punctuation usage) compresses the blank half of opening
punctuation at line start, i.e. 1/2 em — our 1/4 em left half of that blank in
place and pushed lines into over-compression instead.

The product needed a default that reflects current Chinese typesetting
practice *before* the human blind test, so the test can measure the real
candidate rather than a reference preset nobody ships.

## Decision

### A separate `publish-v1` preset

`ParamSet::publish_v1()` is `default-v1` with one change: `tolerance = 1500`.
Everything else (weights, fallback ladder, short-last threshold, features)
stays. `default-v1` remains frozen and selectable for comparisons.

`soma-engine` now ships `publish-v1` by default and accepts both
`preset: publish-v1` and `preset: default-v1`. The scorecard, the corpus
default, and the blind kit's K-P candidate use `publish-v1`.

### Opening punctuation compresses by half an em

`OPEN_TRIM_LEFT` changes from 1/4 em to 1/2 em, matching the blank side of
full-width opening brackets and quotes. Closing (1/2 em) and middle (1/2 em)
punctuation already used the half-width cap; pair compression stays at 1/4 em
because it only affects rare line-start pairs. The rules table change bumps
`rules_version` from 0 to 1, so pages record the new baseline.

### Mixed CJK–Latin spacing stays at 1/4 em

`CJK_LATIN_GAP` was already 1/4 em with 1/16 em stretch and shrink, which is
the `clreq` recommendation; no change.

## Evidence

Corpus (`corpus/zh`, 12 documents, 1214 → 1215 lines):

| metric | default-v1 | publish-v1 |
|---|---|---|
| over-compressed lines | 2 | **0** |
| pages | 48 | 48 |
| `notice` jitter P95 | 1646‰ | **337‰** |
| `technical` jitter P95 | 820‰ | **302‰** |
| widows / orphans | 0 / 0 | 0 / 0 |

The synthetic 40-paragraph scorecard is unchanged (18 pages, cost 790336); its
samples do not hit the tolerance or the opening-punctuation trim.

## Consequences

- `engineHash` changes for every document because the preset id and parameters
  change; `rulesVersion` becomes 1. No schema bump.
- `docs/params.md`, `docs/engine.md`, `docs/corpus-calibration.md` and the
  corpus numbers move to the new defaults.
- The values are provisional: the pre-registered blind test (Gate Q) is the
  human check, and the interview phase may replace `tolerance` or the trim
  ratios. Changing either is a new preset / rules version, not an edit to
  `publish-v1`.

## Alternatives rejected

- **Keep `default-v1` as the shipped default.** Leaves known over-compression
  and a conservative trim that contradicts the half-width cap.
- **Change `default-v1` in place.** Breaks its role as the frozen K-P
  reference used by the kernel tests and the corpus history.
- **Set `OPEN_TRIM_LEFT = 1/2` without a preset change.** The trim table is a
  different layer than the tolerance, but shipping either alone leaves a
  mixed baseline that is harder to describe and to revert.
