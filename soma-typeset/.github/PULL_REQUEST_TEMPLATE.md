## What changed

## Why

Link the issue and, for behavior changes, the ADR.

## Checks

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace`
- [ ] `bash scripts/gates.sh`
- [ ] `bash scripts/studio.sh` (if the change touches the Studio)
- [ ] `bash scripts/web.sh` (if the change touches the browser slice)
- [ ] Updated `docs/debt.md`, the atlas, or the relevant schema document
- [ ] No generated artifacts, fonts, PDFs, `dist/`, or secrets added
