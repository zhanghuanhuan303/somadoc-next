# Blind-test protocol and tooling

Status: the tooling is implemented; **no human blind test has been run**. Gate Q
remains `Inconclusive` until real, pre-registered data exists.

## What is compared

Each sample paragraph is rendered twice by the demo/participant tool:

- **A** — the candidate (Knuth-Plass with `default-v1`);
- **B** — the baseline (Greedy-F, furthest fit).

The evaluator sees the two layouts without algorithm labels and chooses A or B.
The ordering is randomized by the experiment runner; this repository only
consumes the resulting choices.

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
cargo run -p soma-eval --release -- blind preferences.csv
```

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
