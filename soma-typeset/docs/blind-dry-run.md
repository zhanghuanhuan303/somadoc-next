# Blind-test dry run (mechanism check)

Date: 2026-11-20. **No human data was collected.** This run proves the
pipeline works end to end; it says nothing about typographic quality.

## What ran

```sh
# 1. Regenerate the kit (deterministic for this seed).
cargo run -p soma-demo -- --blind-kit corpus/blind/samples.txt \
  --out corpus/blind/kit --seed 20261120 --width-em 40
# -> sheet.html (30 unlabeled pairs), key.csv, ratings.template.csv
# The regenerated kit is byte-identical to the committed one.

# 2. Produce the simulated panel (fixed arithmetic, not people).
python3 tools/dry-run-panel/panel.py   # see corpus/blind/dry-run/README.md

# 3. Analyse it.
cargo run -p soma-eval -- blind \
  corpus/blind/dry-run/simulated-preferences.csv \
  --key corpus/blind/kit/key.csv \
  --out corpus/blind/dry-run/report.md
```

## Result

| metric | value |
|---|---|
| paragraphs | 30 |
| evaluators | 5 |
| A wins (candidate) | 13 |
| B wins (baseline) | 17 |
| ties | 0 |
| unanimous B | 0 |
| point | 0.4333 |
| Wilson 95% | [0.2738, 0.6080] |
| Fleiss' kappa | -0.1232 |
| sample ok | true |
| decision | **inconclusive** |

Exit code `2` (inconclusive), which is the honest outcome: the simulated panel
does not prefer the candidate. The interesting part is that the machinery
behaves — the key swap is applied, the counts add up, the interval brackets the
point estimate, kappa is computed from the raw ratings, and an inconclusive
result is not silently upgraded to a pass.

## What this does **not** say

- It is not evidence that K-P beats greedy on Chinese text.
- It is not a pilot: kappa is negative because the simulated rule is
  anti-correlated once decoded, not because evaluators disagree.
- Gate Q and debt D01 remain **open** until a real panel of ≥30 paragraphs ×
  5 evaluators fills the sheet.

## Reproducibility

`crates/soma-eval/tests/blind_pipeline.rs` loads the same CSV and key in CI and
asserts the counts, the interval bracketing, the marker in the CSV header, and
the `Inconclusive` decision. Any change to the parser, the statistics, or the
decision rule shows up there.
