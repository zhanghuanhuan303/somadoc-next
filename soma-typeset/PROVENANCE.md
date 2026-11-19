# Provenance

This workspace is a **clean-room** implementation of the PRD in `../prd.md` and
the frozen architecture in [`docs/architecture-v2.md`](docs/architecture-v2.md).
It does not include, copy, or reference code from a prior Typst project.

No path or git dependency points at a Typst-based project.

## Public references

- The PRD and this workspace's own architecture/ADR documents.
- The published Knuth-Plass line-breaking algorithm description.
- CLReq / GB/T and publisher practice for Chinese rules, cited by clause; tables
  are not copied wholesale.
- Rust standard library and public crate documentation.

## Legacy quarantine

`crates/soma-core/src/semantic.rs` and `crates/soma-core/src/text.rs` predate
architecture v2 and are retained only because the current K1–K8 tests still
reference them. They are **not part of the frozen architecture**, no new code
may depend on them, and ADR-0008 removes them from the public API in A1.

They are not copied into `soma-doc`. The clean-room DocIR is designed from
scratch against the architecture and ADR-0001/0008.

## Not included

- Any Typst source generation, templates, preview adapters, or `typst*` /
  `typst-pdf` / `typst-svg` / `typst-assets` dependency.
- The old Markdown-to-Typst path.
- Any Typst `World` implementation or font-loading adapter.
- Any code, table, or template from the deleted prior project.

## New code

The following files are new for this workspace:

- `crates/soma-core/src/units.rs`
- `crates/soma-core/src/metrics.rs`
- `crates/soma-core/src/ir.rs`
- `crates/soma-core/src/breaker.rs`
- `crates/soma-core/src/params.rs`
- `crates/soma-core/src/result.rs`
- `crates/soma-doc/src/ids.rs`
- `crates/soma-doc/src/attrs.rs`
- `crates/soma-doc/src/node.rs`
- `crates/soma-doc/src/meta.rs`
- `crates/soma-doc/src/inline.rs`
- `crates/soma-doc/src/block.rs`
- `crates/soma-doc/src/raw.rs`
- `crates/soma-doc/src/doc.rs`
- `crates/soma-doc/src/validate.rs`
- `crates/soma-doc/src/canonical.rs`
- `crates/soma-doc/src/hash.rs`
- `crates/soma-doc/src/text.rs`
- `crates/soma-doc/src/lib.rs`
- `crates/soma-rules/src/lib.rs`
- `crates/soma-eval/src/lib.rs`
- `crates/soma-eval/src/main.rs`
- `crates/soma-demo/src/lib.rs`
- `crates/soma-demo/src/main.rs`
- `crates/soma-wasm/src/lib.rs`
- `crates/soma-wasm/src/main.rs`

Removed in A1 (ADR-0008): `crates/soma-core/src/semantic.rs` and
`crates/soma-core/src/text.rs`. They were early seeds for this workspace, not
copied from anywhere, and their usable behaviour (`plain_text`) moved to
`soma-doc::text`.

The line-breaking algorithm follows the public Knuth-Plass description; no
existing typesetting-engine source was copied. Chinese punctuation and
line-start/line-end rules are a deliberately small first-pass table and will be
replaced by a versioned rule set with cited sources.

## Rule source ledger (current)

The executable inventory is `soma_rules::RULES`
(`crates/soma-rules/src/lib.rs`):

| status | count | notes |
|---|---:|---|
| `Verified` | 8 | segmentation, decimal runs, hard-break IR contract, punctuation hard rules |
| `Tbd` | 0 | no rule is currently provisional |
| `ParamOnly` | 6 | CJK/Latin/space gaps and trim caps |

**Known debt:** the punctuation hard rules are now `Verified` against
GB/T 15834-2011 via W3C clreq §6.1.1 (basic, no-line-start / no-line-end
punctuation). The remaining debt is narrower: the trim *ratios* are
`ParamOnly` and still need calibration, so Gate Q may cite the hard rules but
must not treat the trim amounts as sourced. Recorded in `docs/k6-report.md`
and `docs/k8-report.md`.

## Policy

- Cite clauses, do not reproduce tables. The project references the relevant
  clause or paragraph; it does not copy a standards table wholesale.
- `Tbd` rules are soft rules only: they may emit `Allowed` + penalty/trim, never
  `Forbidden` / `Forced`. A hard rule requires `Verified` status. Promoting a
  rule to `Verified` requires a cited clause in its `source` field.
- GB/T is a recommended standard; only the applicable normative text is cited.
  Any quoted excerpt must be short, attributed, and reviewed for license
  compatibility before it is committed.
- Publisher-practice rules are recorded as practice, not as a standard; their
  provenance is the sample/edition, not a normative clause.
- New code must follow architecture v2 and the ADRs; deviations need a new ADR.
