# ADR-0012: Table-backed font metrics

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0010 (borrow-first), `docs/proposal-effect-max.md` E1
- Supersedes: nothing. Fills the `Metrics` seam that ADR-0005 left to the host.

## Context

Every width in the system came from `SyntheticMetrics`: one width class for CJK,
one for Latin, one for punctuation. Line breaks, trim, boundary deviation, and
line boxes were therefore measured against a font that does not exist. No visual
claim was testable, and style sizes did not affect advances at all.

The kernel already reads metrics through a seam:

```rust
pub trait Metrics: Sync {
    fn advance(&self, text: &str, style: StyleId) -> Width;
    fn line_metrics(&self, style: StyleId) -> LineMetrics;
    ...
}
```

## Decision

Ship a committed integer table and parse it at runtime; never open a font in the
shipped crates.

- **Format `soma.metrics/1`**: little-endian, fixed fields, all lengths in
  1/4096 em. Per face: ascent/descent/leading, space advance, a default advance,
  uniform codepoint blocks (CJK is 1 em across the block), and a sorted
  exception table. Lookups are integer-only and identical on every target.
- **Producer**: `tools/metrics-gen` (`publish = false`, std) reads `head`,
  `hhea`, `OS/2`, `hmtx`, `maxp`, and `cmap` from a TTF/OTF or a collection
  index. It extracts numbers only — no outlines. Output is committed.
- **Faces**: Noto Serif CJK SC and Noto Sans CJK SC (SIL OFL 1.1), face index 2
  of each collection. Coverage: Latin/symbols U+0020–U+2FFF, CJK punctuation and
  kana U+3000–U+30FF, Ext A, CJK U+4E00–U+9FFF, compatibility, fullwidth.
- **Adapters** in `soma-layout`:
  - `TableMetrics` — one face, one size (demos, kernel experiments);
  - `StyledMetrics` — `StyleId` → (face, font size) through `StyledDoc`, so a
    2em heading measures at 2em. `sans`/`mono`/`hei`/`gothic` select face 1.
- **Identity**: `Metrics` gains defaulted `version()` and `hash()`; the page
  result carries `metricsVersion` and `metricsHash`. A table swap is visible in
  canonical bytes even when the format version is unchanged.

## Consequences

- `soma.pages/2` adds `metricsHash`. The WASI fixture changes; the gate keeps
  comparing native and wasm bytes.
- Every snapshot generated against `SyntheticMetrics` had to be regenerated once.
  Kernel tests keep using the synthetic adapter: the kernel is metric-agnostic.
- wasm builds parse the same table, so cross-target bytes match by construction.
- The table is ~31 KB for two faces. It is data, not code, and does not grow the
  no_std dependency graph: `soma-metrics` has **zero** dependencies.
- Known approximation: bold uses regular advances (Latin bold differs slightly;
  CJK bold does not). Tracked as D61.

## Alternatives rejected

- **Runtime font parsing in the core.** Pulls a font parser (and a licence,
  filesystem, and platform API) into a `no_std`, `forbid(unsafe_code)` crate.
- **Float metrics or `f32` scaling.** Breaks cross-target hash equality.
- **Generating the table at build time.** Makes builds depend on a font file
  being present; committed bytes are reproducible offline.
