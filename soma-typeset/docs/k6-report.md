# K6 report

Status: **K6 engineering slice complete.** Performance is inside the native
budget with the conservative atom-count window; the fallback ladder is
implemented; the benchmark/sensitivity harness is reproducible. Tier A
calibration is **not** complete and Gate Q remains **Inconclusive**, because a
quality-labeled tuning corpus and human blind test do not exist yet.

## Delivered

### 1. Conservative atom-count window (performance)

The DP now computes a safe window `[k_min, k_max]` of atom counts that can
possibly form a justified line:

- underfull infeasibility: even the maximum natural width for `k` atoms,
  with maximum stretch (including `emergency_stretch`), cannot reach the
  smallest target;
- overfull infeasibility: even the minimum width for `k` atoms, minus the
  maximum possible trim, remains beyond the largest target plus the maximum
  shrink.

The window is derived from global maxima/minima, so it may be loose, but it
cannot discard a feasible justified line. It is disabled when
`allow_emergency` is true, because emergency lines may be arbitrarily short or
overfull. Paragraph-end and forced-segment-end lines bypass the window.

Differential checks:

- `brute_force_matches_dp` / `brute_force_matches_dp_across_lengths`;
- `brute_force_matches_dp_with_forced_breaks`;
- `pruning_matches_brute_force_on_varied_widths`;
- `pruning_reduces_the_candidate_count`.

Active-node deactivation is still not implemented. The width window is enough
for the measured budget and avoids the trim/monotonicity risk.

### 2. Fallback ladder

`Normal → relaxed without emergency flags → emergency flags → ForbiddenRescue`.
The relaxed stage uses `emergency_tolerance` and `fallback.emergency_stretch`
(the hard-coded values were removed). `fallback.allow_overfull` gates terminal
and zero-shrink overfull lines; `fallback.allow_forbidden` gates rescue.
`FallbackStage::Overfull` remains reserved; in this cost model the emergency
stage already covers the cases it would allow.

### 3. Evaluation harness (`soma-eval`)

- deterministic corpora: pure CJK, mixed CJK/Latin/digits, punctuation-dense;
- sizes 100/500/1,000/2,000/4,000 atoms;
- P50/P95 over repeated runs, evaluated candidates, flags, trim total,
  boundary-deviation total, peak RSS on Linux;
- `bench --check` against the proposal budgets (2,000 ≤ 20 ms, 4,000 ≤ 80 ms);
- `sweep-tolerance` for `(tolerance, badness_scale)`;
- `sweep-weight` for line/break_penalty/compress_amount/compress_count/
  boundary_deviation/short_last_scale;
- `inspect` for per-line diagnosis;
- blind-test statistics (Wilson + Fleiss κ) used by K8.

### 4. Length policy

`max_kp_atoms` remains `None` in `default-v1`. Measurement after the width
window puts 4,000 atoms well below the native budget, so the default does not
need a Greedy-C fallback. The policy remains implemented and tested:
`length_limited = true`, `breaker_used = GreedyC`.

If a WASM timing (not just a compile check) later exceeds the budget, set
`max_kp_atoms = Some(2000)`; this is a preset change, not a model change.

### 5. Rule inventory

`soma_rules::RULES` contains 8 `Verified`, 0 `Tbd`, and 6 `ParamOnly` rows. The
punctuation hard rules are `Verified` against GB/T 15834-2011 via W3C clreq
§6.1.1 (basic, no-line-start / no-line-end punctuation). The trim ratios remain
`ParamOnly` and uncalibrated, so they are not a Gate Q claim.

## Benchmark

Native release, synthetic metrics, 5 repeats. `scripts/bench.sh` runs the same
command with `--check`.

| corpus | atoms | lines | P50 ms | P95 ms | evaluated | stage | overfull |
|---|---:|---:|---:|---:|---:|---|---:|
| pure-cjk | 100 | 5 | 0.008 | 0.009 | 90 | normal | 0 |
| pure-cjk | 500 | 25 | 0.058 | 0.059 | 1,580 | normal | 0 |
| pure-cjk | 1,000 | 50 | 0.124 | 0.133 | 3,580 | normal | 0 |
| pure-cjk | 2,000 | 100 | 0.256 | 0.267 | 7,580 | normal | 0 |
| pure-cjk | 4,000 | 200 | 0.523 | 0.533 | 15,580 | normal | 0 |
| mixed | 100 | 7 | 0.012 | 0.013 | 260 | normal | 6 |
| mixed | 500 | 34 | 0.062 | 0.067 | 1,544 | normal | 33 |
| mixed | 1,000 | 67 | 0.126 | 0.127 | 3,140 | normal | 66 |
| mixed | 2,000 | 134 | 0.253 | 0.266 | 6,344 | normal | 133 |
| mixed | 4,000 | 267 | 0.569 | 0.583 | 12,740 | normal | 266 |
| punct-dense | 100 | 5 | 0.006 | 0.007 | 16 | normal | 1 |
| punct-dense | 500 | 27 | 0.147 | 0.156 | 4,403 | emergency | 0 |
| punct-dense | 1,000 | 54 | 0.290 | 0.297 | 8,906 | emergency | 0 |
| punct-dense | 2,000 | 107 | 0.580 | 0.583 | 17,929 | emergency | 0 |
| punct-dense | 4,000 | 214 | 1.216 | 1.227 | 36,028 | emergency | 0 |

Peak RSS: **~3.7 MiB**. `bench --check`: **PASS** (native budgets). P95
varies by a few percent between runs; the table is one representative run.

Before the width window, pure CJK 4,000 atoms was ~122 ms and punctuation-dense
was ~81 ms. The window reduced evaluated candidates from millions to tens of
thousands.

The mixed corpus shows a high `overfull` count. That is expected: long Latin
runs are unbreakable and the synthetic corpus repeats them; the flag is a
quality diagnostic, not a fallback stage.

The punct-dense corpus enters `Emergency` because a few lines need the 3em
emergency stretch, but the result has **0 emergency-flagged lines**. The first
relaxed attempt (no emergency flags) succeeds; it is still recorded as the
Emergency stage.

## Sensitivity (diagnostic, not calibration)

`(tolerance, badness_scale)` at 1,000 atoms, 3 corpus kinds:

| tolerance | badness | lines | overfull | trim total | cost |
|---:|---:|---:|---:|---:|---:|
| 1,000 | 50 | 177 | 0 | 18,432 | 3,574 |
| 1,000 | 100 | 177 | 0 | 18,432 | 4,031 |
| 2,000 | 100 | 171 | 66 | 135,168 | 15,359 |
| 3,000 | 100 | 171 | 66 | 135,168 | 15,359 |
| 4,000 | 100 | 167 | 87 | 161,792 | 79,432 |

Reading:

- At `tolerance = 1000`, the breaker adds lines and avoids trim/overfull.
- At `tolerance >= 2000`, it uses much more trim and fewer lines; 2,000 and
  3,000 are identical on this corpus.
- `tolerance` and `badness_scale` are coupled, as the model warned. A higher
  badness scale raises cost but does not change the chosen breaks on this
  corpus.

One-dimensional sweeps at 1,000 atoms:

| parameter | values | lines | trim total | cost |
|---|---:|---:|---:|---:|
| `compress_amount` | 5/10/20/40 | 171 | 135,168 | 14,831 → 16,019 |
| `compress_count` | 0/5/10/20 | 171 | 135,168 | 15,029 → 16,349 |
| `boundary_deviation` | 0/25/50/100 | 171 | 135,168 | 14,435 → 16,349 |
| `short_last_scale` | 100k/300k/1M | 171 | 135,168 | 15,359 |

**Conclusion:** these weights change reported cost but not the chosen layout on
this synthetic corpus. That is not evidence that they are unimportant; it means
the corpus cannot calibrate them. Tier A calibration needs a corpus with
quality labels, and Gate Q must stay Inconclusive until then.

## Fallback tests

`crates/soma-core/tests/k6_fallback.rs` (4):

1. `forbidden_rescue_is_the_last_resort`
2. `forbidden_rescue_is_skipped_when_disallowed`
3. `normal_stage_never_violates_forbidden_boundaries`
4. `emergency_stage_reports_its_lines`

## Verification

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 110 passed
  wasm32 check: pass
  demo render: pass
  soma-core dependency boundary: pass
```

Tests: 25 core unit + 56 core integration + 16 rules + 9 eval + 3 demo + 1
wasm fixture = 110.

## Deferred / honest gaps

- No quality-labeled corpus; no Tier A calibration; no human blind test.
- No WASM execution timing (only `wasm32-unknown-unknown` compile check).
- `FallbackStage::Overfull` reserved; emergency covers the current cases.
- Active-node deactivation not implemented (not needed for the measured budget).
- Punctuation trim ratios are still `ParamOnly`; calibration remains debt.
