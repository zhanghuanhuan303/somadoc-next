# ADR-0022: Bold faces in the metrics table

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0012 (table-backed metrics), `docs/debt.md` D61
- Extends: `soma.metrics/1` data and `StyledMetrics`. No template or page
  schema change.

## Context

The metrics table shipped only Regular faces. `weight: bold` therefore measured
with regular advances: CJK is unaffected (ideographs are one em in every
weight), but Latin emphasis kept regular widths and line breaks could be wrong
for bold runs. D61 recorded this.

The blob format already stores `weight` and `slant` per face, and
`tools/metrics-gen` already accepted `family:weight:path#index`, so the data
model did not need a new version.

## Decision

### Four faces, fixed order

`crates/soma-metrics/data/noto-cjk.smt` now contains four faces, in this order:

| index | family | weight |
|---|---|---|
| 0 | serif | regular |
| 1 | sans | regular |
| 2 | serif | bold |
| 3 | sans | bold |

Generated from Noto Serif/Sans CJK SC Regular and Bold (OFL), face index 2 of
each collection. The blob is 63,212 bytes; `soma.metrics/1` is unchanged.

### Selection by weight

`StyledMetrics::face_of` maps a `ComputedStyle` to a face:

1. family (`serif` vs `sans`-like names) selects the base 0 or 1;
2. `weight == bold` adds 2;
3. if the exact face is missing (an older 2-face table), fall back to the same
   family's regular face, then to face 0.

`TableMetrics` keeps its explicit `face` argument, so callers can pick a bold
face directly.

### What this does not do

CJK fonts have no italic faces. `slant: italic` therefore keeps the upright
advance; synthetic oblique (a renderer transform) is not part of the box model
and remains out of scope. The italic half of D61 stays open, recorded as such.

## Consequences

- D61 closes for bold; the table grows from 31,612 to 63,212 bytes.
- `metricsHash` changes, so page canonical bytes change for every document (the
  hash is data, not a schema). No schema bump.
- The evaluation templates do not set `weight: bold` on `strong`, so the
  scorecard and corpus metrics are unchanged by this step.
- `soma-metrics` tests assert four faces and that bold Latin advances differ;
  `soma-layout` has a test that a bold `strong` run measures with the bold face.
- Existing 2-face tables still load: the parser reads `face_count` and the
  family fallback keeps them usable.

## Alternatives rejected

- **Synthetic bold (stroke widening) in the renderer.** No metric change and a
  visual fake; it would not fix line breaking, which is the actual debt.
- **A second metrics blob for bold.** Two hashes and two lookups for what the
  face table already models.
- **Variable-font axes.** The extractor reads static instances; variable
  instancing is a tooling project, not a metrics-table change.
