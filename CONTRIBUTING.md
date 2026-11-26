# Contributing

The project lives in [`soma-typeset/`](soma-typeset/). Read the full guide at
[`soma-typeset/CONTRIBUTING.md`](soma-typeset/CONTRIBUTING.md); all commands run
from that directory unless stated otherwise.

The short version: behavior changes start with an ADR, determinism and the
gate baselines are not optional, and no generated artifacts, fonts, PDFs,
`dist/` or secrets belong in a commit.

```sh
cd soma-typeset
cargo fmt --all -- --check
cargo test --workspace
bash scripts/gates.sh
```
