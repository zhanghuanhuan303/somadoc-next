# ADR-0072: the emergency stage gets a width window (D94)

- Status: Accepted
- Date: 2026-11-21
- Related: ADR-0007 (kernel frozen), ADR-0071 (browser slice), D91, D94
- Touches: `soma-core` (`Shaped::line_length_bounds`), `soma-web` (fallback
  policy), `docs/debt.md`

## Context

`Shaped::line_length_bounds` bounds how many atoms a line may hold, so the DP
scan stays proportional to the visible width of a line rather than to the
paragraph. It had one early return:

```rust
if allow_emergency {
    // Emergency lines may be arbitrarily short or overfull ...
    // Keep the exhaustive scan for the fallback path.
    return (1, n);
}
```

The reasoning was that the emergency stage relaxes two things — short lines are
accepted, and (with `allow_overfull`) overfull lines are accepted — so the width
window "does not apply". The first half is true. The second half is what made
the fallback quadratic.

Measured on the `mixed` corpus (2,000 atoms, `SyntheticMetrics`, `default-v1`):

| measure | stage | candidates | wasm (node) | wasm (Chromium) |
|---|---|---|---|---|
| 18 em | Emergency | **1,255,320** | 29.0 ms | 41.1 ms |
| 20 em | Emergency | **1,255,320** | 28.9 ms | 40.5 ms |
| 21 em | Normal | 3,278 | 0.78 ms | 0.70 ms |
| 40 em | Normal | 61,722 | 2.38 ms | 3.20 ms |

The cliff is not wasm-specific: D91's native baseline (20.23 ms at 2,000 atoms)
was measuring this path all along. Its debt entry already said "1.28M line
evaluations" — those evaluations were the emergency stage scanning every start
for every end.

There was a second, subtler contributor: the relaxed stage uses
`emergency_tolerance = 10.0`. The window arithmetic multiplies the tolerance by
the per-boundary shrink, so a tolerance above ~32 made `coefficient <= 0` and the
bound vacuous — the *normal* stage with a relaxed tolerance scanned quadratically
as well (1,206,741 candidates at `tolerance = 10_000`).

## Decision

Bound the overfull direction in every stage:

```rust
// An overfull line (excess beyond the available shrink) is rejected unless the
// emergency *and* overfull flags are both set, so the ratio bound that actually
// admits a candidate is at most 1.0 (full shrink) whenever those flags are not
// both present.
let tolerance_eff = tolerance.min(1_000);
let extra_overfull = if allow_emergency && allow_overfull {
    emergency * 1_000          // the declared emergency_stretch, and no more
} else {
    0
};

let coefficient = min_atom * 1000 - tolerance_eff * max_shrink;
// ... same loop as before, with `- extra_overfull` in the margin ...
```

and keep the emergency stage's lower bound at 1 (short lines are legal there).

Why this is behaviour-preserving where it matters:

- `eval_line` rejects an overfull line unless **both** `allow_emergency` and
  `allow_overfull` are set. With `tolerance_eff = min(tolerance, 1000)` the bound
  is exactly the rejection test, so no admissible candidate is dropped.
- When both flags are set, an arbitrarily long overfull line *was* admissible.
  The window now caps the excess at the declared `emergency_stretch`, which is
  the same number that already governs the underfull direction. A line over
  target by more than that is not a typographic outcome the engine should
  choose; the escape hatch stays open for the case it exists for (one
  unbreakable atom), because the window always keeps at least one atom.

## Consequences

| measure | before | after |
|---|---|---|
| candidates @ 20 em | 1,255,320 | 30,316 |
| wasm @ 20 em (Chromium) | 40.5 ms | ~2.5 ms |
| break sequence | — | **identical** (144 lines, same ends) |
| `ALL,48,599,…` corpus baseline | — | **unchanged** |
| 770 workspace tests | — | **unchanged** |

The `mixed` benchmark at 20 em still reports `fallback_stage = Emergency` (the
corpus genuinely cannot be justified inside the tolerance at that measure), but
the plan it finds contains no overfull line, which is what made the tighter
window possible.

Residual risk: a paragraph that could only be solved by an overfull line longer
than `target + emergency_stretch` will now be solved differently (or, if nothing
else is feasible, one atom per line). No document in `corpus/zh` or the golden
fixtures takes that path; `kp_window.rs` pins both sides of the behaviour.

## Alternatives

- **Leave the emergency scan exhaustive and make the host downgrade to greedy.**
  That is still the host policy in `soma-web` (because a 3-4x slower emergency
  run is not worth it in a browser), but leaving a kernel path that is
  quadratic in paragraph length is a trap for every other caller. Rejected.
- **Cap the number of evaluated candidates with a budget.** Non-deterministic:
  the result would depend on where the budget ran out. Rejected.
- **Only bound when `allow_overfull == false`.** Provably safe, but it fixes
  nothing for the default preset, which is the case that hurts. Rejected.

## Verification

- `crates/soma-core/tests/kp_window.rs`:
  `the_emergency_stage_is_not_a_full_quadratic_scan` (stage is Emergency,
  candidates < 200,000, no overfull lines) and
  `an_unbreakable_atom_still_gets_a_line` (the escape hatch survives).
- `scripts/gates.sh` — corpus baseline `ALL,48,599,1213,0,0,3,0,0,3,486,1580,377,9`
  unchanged, all 12 gates green.
- `bash scripts/web.sh` — W4 measures the slice in node and Chromium.
