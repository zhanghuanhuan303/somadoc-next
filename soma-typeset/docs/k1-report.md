# K1 report

Status: **K1 green**. The parameter contract, breaker cost model, brute-force
reference, and public fixtures are implemented; the repository gates pass.

## Benchmark decision

Run:

```sh
cargo run -p soma-core --example bench --release
```

Observed on the development machine (native release, synthetic metrics, 20em
lines, pure CJK):

```text
atoms=2000 lines=100 elapsed_ms=1.75
atoms=4000 lines=200 elapsed_ms=6.78
```

Decision: keep `default-v1.max_kp_atoms = None`. The n=4,000 case is well under
the K1 provisional budget, so the Greedy-C length fallback is not enabled by
default. K6 will measure wasm and real mixed text before revisiting this.

## What K1 locked

- `ParamSet::default_v1` values and `param_hash` (FNV-1a over `BreakParams` +
  feature flags).
- Validation ranges and cross-field checks.
- `BreakConfig` holds `ParamSet`; `CostWeights` is removed.
- `line`, normalized `break_penalty`, normalized `badness`, and configurable
  `short_last` terms.
- Non-Justify alignment returns `UnsupportedAlign`.
- DP vs brute-force equality for n=3..=10 (helper supports n<=16).
- Tie-break favors the longer last line on equal cost/line-count.
- 10 public fixture scenarios in `crates/soma-core/tests/k1_fixtures.rs`.

## Deferred to K2/K3/K5

- `LayoutResult` stage fields (`breaker_used`, `fallback_stage`,
  `length_limited`, `cjk_latin_adjustments`, line-height fields).
- Edge trim, boundary-deviation aggregation, style/line-height output.
- Forced breaks/segments and `forced_segment_no_indent`.
- Real wasm and mixed-text benchmarks.

## Verification

```text
scripts/gates.sh
  fmt: pass
  clippy -D warnings: pass
  tests: 49 passed
  wasm32 check: pass
  soma-core dependency boundary: pass
```
