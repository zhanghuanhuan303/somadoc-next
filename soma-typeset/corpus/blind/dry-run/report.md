# Blind test report

- preferences: `corpus/blind/dry-run/simulated-preferences.csv`
- key: `corpus/blind/kit/key.csv`
- protocol: 30 paragraphs x 5 evaluators, majority per paragraph
- candidate: A = Knuth-Plass `publish-v1`; baseline: B = Greedy-F

**Decision: `inconclusive`**

| metric | value |
|---|---|
| paragraphs | 30 |
| evaluators | 5 |
| A wins | 13 |
| B wins | 17 |
| ties | 0 |
| unanimous B | 0 |
| point | 0.4333 |
| Wilson 95% | [0.2738, 0.6080] |
| Fleiss kappa | -0.1232 |
| sample ok | true |

Rule: pass needs >= 30 paragraphs, no unanimous B, point >= 0.60,
Wilson lower bound > 0.50, and kappa >= 0.20. Fail on a unanimous B
paragraph or a Wilson upper bound below 0.50. Anything else is
inconclusive and blocks only the public quality claim.
