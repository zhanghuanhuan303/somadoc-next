# Blind-test protocol and tooling

Status: the kit generator and the analysis are implemented; **no human blind
test has been run**. Gate Q remains `Inconclusive` until real, pre-registered
data exists.

## What is compared

Each sample paragraph is rendered twice by the demo/participant tool:

- **A** — the candidate (Knuth-Plass with `publish-v1`);
- **B** — the baseline (Greedy-F, furthest fit).

The evaluator sees the two layouts without algorithm labels and chooses A or B.
The sheet generator randomizes the side deterministically from a seed, and the
analysis step undoes that mapping with the kit's key, so the report keeps
counting A as the candidate.

## Generating the kit

```sh
cargo run -p soma-demo -- --blind-kit corpus/blind/samples.txt \
  --out corpus/blind/kit --seed 20261120 --width-em 40
```

`corpus/blind/samples.txt` is the frozen 30-paragraph set. The command writes:

- `sheet.html` — 30 groups, each with two unlabeled renderings. Algorithm names
  never appear in the file;
- `key.csv` — `paragraph,kp_side`: which side (A or B) holds the K-P candidate.
  Keep this away from evaluators;
- `ratings.template.csv` — `paragraph,evaluator,choice` with 30 × 5 blank rows.

The order is deterministic for a given seed. Do not change the samples or the
seed after collecting ratings; if you must, regenerate the kit and re-run.

## CSV format

```csv
paragraph,evaluator,choice
0,0,A
0,1,B
...
```

- `paragraph` and `evaluator` are integer labels.
- `choice` is `A` or `B` (case-insensitive).
- A header row is ignored. Empty lines and `#` comments are ignored.
- Every paragraph must have the same number of ratings. The canonical protocol
  is **30 paragraphs × 5 evaluators**.

Run:

```sh
cargo run -p soma-eval --release -- blind preferences.csv --key key.csv
```

`--key` is optional for hand-built matrices where A always means the candidate.
When the kit placed the candidate on side B for a paragraph, the tool swaps that
paragraph's A/B before computing the report.

Exit codes: `0` pass, `1` fail, `2` inconclusive.

## Decision rule

The report computes:

- per-paragraph majority (A wins / B wins / tie);
- `point = a_wins / paragraphs` (ties count against A);
- Wilson 95% interval over paragraphs;
- Fleiss' kappa over the A/B ratings;
- the number of paragraphs where every evaluator chose B.

The decision is:

- **Pass** only when all hold:
  - at least 30 paragraphs;
  - no unanimous B paragraph;
  - point estimate ≥ 0.60;
  - Wilson lower bound > 0.50;
  - Fleiss' kappa ≥ 0.20.
- **Fail** when there is a unanimous B paragraph or the Wilson upper bound is
  below 0.50.
- **Inconclusive** otherwise. Inconclusive does not block technical delivery; it
  blocks a public quality claim.

The tool does not silently turn “Inconclusive” into “Pass”.

## Pre-registration

Before opening the formal set:

1. Freeze the sample set, evaluator pool, and randomization seed.
2. Run a pilot of 5–8 evaluators × 8–10 paragraphs and measure kappa.
3. If kappa < 0.20, calibrate the judgment examples and re-run the pilot; do not
   proceed to the formal set with an uncalibrated rubric.
4. If the 30-paragraph Wilson interval crosses 50%, the pre-registered extension
   is 60 paragraphs. If it still crosses 50%, record **Inconclusive**; do not
   collect more data to chase significance.

## Honest limits

- A synthetic preference fixture is not a quality claim. The unit tests in
  `soma-eval` construct synthetic choice matrices only to test the statistics.
- The tool reports `kappa = undefined` when every rating is identical; that
  case is treated as `Inconclusive`, not as perfect reliability.
- The formal result and the exact CSV should be committed with the report so the
  numbers are reproducible.
