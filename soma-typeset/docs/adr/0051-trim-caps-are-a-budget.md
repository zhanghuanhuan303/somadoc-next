# ADR-0051: a trim cap is a budget, not a measurement

- Status: Accepted
- Date: 2026-11-20
- Closes: D04
- Related: ADR-0037 (ink comes from the outline), ADR-0014 (trim scaling),
  `docs/trim-calibration.md`, `docs/debt.md` D04
- Touches: nothing. This ADR records a measurement and decides **not** to
  change the shipped values.

## Context

D04 listed six `ParamOnly` ratios in `soma-rules` as "not calibrated":
`OPEN_TRIM_LEFT`, `CLOSE_TRIM_RIGHT`, `MIDDLE_TRIM_LEFT`, `MIDDLE_TRIM_RIGHT`,
`LATIN_TRIM`, `PAIR_TRIM`. All were round numbers (1/2, 1/2, 1/2, 1/2, 1/8,
1/4) with no measurement behind them.

Since ADR-0037 the renderer takes a mark's side bearing from the font's own
outline when the metrics table carries it, so the constants are only the
fallback for tables that do not. That suggested a calibration with no human in
the loop: measure the empty space each punctuation class leaves in the faces we
ship, and set the fallback to the measurement.

## Decision

### 1. Measure first

`cargo run -p soma-metrics --example punct -- <table>.smt` reads the ink the
metrics table already stores and prints the median gap per class. On the
MiSans + IBM Plex table:

| class | median gap (em) | shipped constant |
| --- | --- | --- |
| opening, left | 0.627 | 0.5 |
| closing, right | 0.627 | 0.5 |
| middle, left | 0.184 | 0.5 |
| middle, right | 0.702 | 0.5 |
| half-width, left | 0.039 | 0.125 |
| half-width, right | 0.067 | 0.125 |

The shipped values are wrong as *measurements* — a middle mark leaves a fifth of
an em on its left, not half.

### 2. Then test the measurement as a setting

Setting the six constants to the measured values (rounded to binary fractions,
`PAIR_TRIM` to zero because the class trim already spends the measured left
gap) and running the Gate Q′ scorecard on `corpus/zh` with `publish-v1`:

| metric | shipped | measured |
| --- | --- | --- |
| lines | 1213 | 1213 |
| over-compressed lines | 0 | 0 |
| emergency lines | 3 | 3 |
| fitness jumps | **3** | **5** (`notice` 0 → 2) |
| ratio jitter p95 | **368** | **428** |

Pages and line count do not move; the two break-quality guards both get worse.
ADR-0042 established that 3 is the floor for this corpus, so 5 is a real
regression, not noise.

### 3. Keep the shipped values, and say why

`compress_left` / `compress_right` are not "how much space is empty". They are
*how far this atom may be pulled in when the line has nowhere else to go*, and
the actual glyph displacement is clamped per glyph by the outline ink
(ADR-0037). A generous budget therefore cannot push a mark into its neighbours —
but a tight budget takes away the breaker's last resort, and it responds by
choosing worse breakpoints.

That makes the two numbers different quantities:

* **ink gap** — a fact about the font, used at render time as a hard clamp;
* **trim cap** — a policy about how hard a line may be squeezed, used by the
  breaker.

Calibrating the second from the first confuses them, and the experiment above
is what that confusion costs.

## Consequences

- The constants keep their values; D04 closes as "measured, and deliberately
  unchanged", which is a stronger statement than "uncalibrated".
- `docs/trim-calibration.md` records the numbers, the command and the
  experiment, so the next person does not repeat it blind.
- `soma-metrics` gains an example. It pulls `soma-core` and `soma-rules` as
  **dev**-dependencies only; the library still has no dependencies at all.
- The remaining honest gap is intra-line pair compression (clreq §6.3.2.2): the
  model has no intra-line trim, so `PAIR_TRIM` stays a rough allowance for the
  edge case.

## Alternatives

- **Adopt the measured values and relax the guard.** Rejected: the guard exists
  because a fitness jump is visible in the set text, and there is no human
  preference data (D01) that could justify trading it away.
- **Keep both sets and add a preset.** A `measured-v1` preset with tighter
  trims is possible and cheap, but nothing in the evaluation asks for it; add it
  when a document actually wants tighter edge compression.
- **Derive the cap from the ink at build time** (per font, into the metrics
  table) instead of a constant. That is what ADR-0037 already does at render
  time; doing it again for the breaker would need the metrics table inside the
  rules layer, which is the layering ADR-0010 forbids.
