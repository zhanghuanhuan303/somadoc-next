# K8 report

Status: **K8 integration slice complete.** Rules → eval → demo is wired, the
blind-test statistics tool is implemented, and the gates include a demo smoke
test. The canonical schema remains `soma.layout/6`; K8 added no result fields.
Gate Q is still **Inconclusive** because no human blind test was run.

## Delivered

### 1. `soma-demo`

A static HTML renderer that consumes `LayoutResult` and never re-breaks text.

- Renders Knuth-Plass and Greedy-F side by side.
- Each line is a relatively positioned block; each atom is an absolutely
  positioned `<span>` with `left` from `AtomBox.x` and `white-space: pre`.
- Line height comes from `Line.height`.
- Overfull / emergency lines are visibly marked.
- Trim, target, natural, adjustment, and ratio are emitted as data attributes.
- The canonical JSON for the K-P result is included in a `<details>` block.
- HTML escaping is tested.

Run:

```sh
scripts/demo.sh
# or
cargo run -p soma-demo --release -- input.txt --width-em 20 > target/demo.html
```

The generated page is self-contained; no fonts or scripts are fetched.

### 2. `soma-eval`

K6's harness is now part of the workspace and is also the K8 evaluation seam:

- `bench` — P50/P95, evaluated candidates, flags, trim/boundary totals, RSS,
  native budget check.
- `sweep-tolerance` — `(tolerance, badness_scale)` grid.
- `sweep-weight` — one Tier A weight at a time.
- `inspect` — per-line trace for a corpus sample.
- `blind` — Wilson interval + Fleiss' kappa + decision.

Scripts:

```sh
scripts/bench.sh       # --repeats 5 --check
scripts/sweep.sh       # sweep-tolerance
scripts/demo.sh
```

### 3. Blind-test statistics

`docs/blind-test.md` records the protocol. The tooling:

- parses `paragraph,evaluator,choice` CSV;
- computes per-paragraph majority, point estimate, Wilson 95% interval,
  Fleiss' kappa, and unanimous-baseline count;
- returns `pass` / `inconclusive` / `fail` with exit codes 0 / 2 / 1.

No real preference data is included. The synthetic matrices in `soma-eval`
unit tests only test the statistics.

### 4. Rules integration

- `paragraph_from_lines` compiles hard-broken source lines into `Forced`
  boundaries.
- `RULES` is the rule-status inventory (8 `Verified`, 0 `Tbd`, 6 `ParamOnly`).
- The demo and eval both consume `soma-rules`, so the end-to-end path is
  exercised in gates and tests.

### 5. Cross-target hash (`soma-wasm`)

The new `soma-wasm` fixture runner compiles a hard-broken, punctuation-heavy
paragraph and prints canonical JSON. The same source is built natively and for
`wasm32-wasip1`; `scripts/wasm-hash.sh` runs the WASI binary with Node and
compares the two byte streams with `cmp`. The gate passes.

```text
native/wasm canonical bytes match (4901 bytes)
```

The WASI binary is 115 KiB, 44,489 bytes gzip. This includes `std`, the rules
compiler, and formatting; it is not the final browser/core-only size. It is
recorded as a cross-target proof and a rough size datapoint, not as the Gate D
budget measurement.

### 6. Gates / regression

`scripts/gates.sh` now also runs:

- a demo smoke check (`soma-demo` → temporary HTML);
- the native/`wasm32-wasip1` canonical-byte comparison.

## Tests

- `crates/soma-demo/src/lib.rs`: 3 tests.
- `crates/soma-eval/src/lib.rs`: 9 tests.
- `crates/soma-wasm/src/lib.rs`: 1 test (fixture deterministic + forced end).
- `crates/soma-core/tests/k6_fallback.rs`: 4 tests.
- Existing K1–K5 suites still pass.

Total: **116 tests**.

## Gate status

| Gate | Status | Notes |
|---|---|---|
| Gate T | **Pass** on the tested surface | `fmt`, `clippy -D warnings`, 116 tests, wasm32 compile check, native/`wasm32-wasip1` byte-identical hash, dependency boundary, demo smoke |
| Gate Q | **Inconclusive** | No pre-registered human blind test; no quality-labeled tuning corpus |
| Gate D | Partial | demo + one-command eval/bench; browser/core-only WASM size and first-paragraph latency not measured; the WASI fixture is 44 KB gzip |

Gate Q is not a failure; it is an honest “not yet demonstrated” status.

## Example commands

```sh
bash scripts/gates.sh
bash scripts/wasm-hash.sh
scripts/demo.sh
scripts/bench.sh
cargo run -p soma-eval --release -- inspect --kind mixed --atoms 500
cargo run -p soma-eval --release -- blind preferences.csv
```

## Deferred

- Markdown front end (`soma-md`): hard breaks can be represented via
  `paragraph_from_lines`, but Markdown syntax parsing is not implemented.
- Browser `soma-wasm` package (wasm-bindgen) and core-only gzip measurement;
  the current `soma-wasm` is a WASI/native hash fixture including `std`.
- Real font metrics table; the demo and eval still use `SyntheticMetrics`.
- Pagination, incremental layout, real first-letter drop caps.
- The human blind-test execution and Gate Q claim.
- Trim-ratio calibration for the `ParamOnly` gap/trim rows.
- WASM execution timing (the gate only proves it compiles).
