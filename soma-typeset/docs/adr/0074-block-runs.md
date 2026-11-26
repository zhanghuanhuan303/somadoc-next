# ADR-0074: blocks cover runs, not holes

- Status: Accepted
- Date: 2026-11-21
- Related: ADR-0039 (face selection), ADR-0073 (CFF ink), D96, `soma.metrics/5`
- Touches: `tools/metrics-gen`, `crates/soma-metrics/data/noto-cjk.smt`

## Context

A `blocks` entry says "every codepoint in this range has this advance". `Face::has`
reads it as *coverage* as well as a width, which is what makes the table small:
Noto CJK SC covers about 32,500 codepoints, and one block can stand in for the
40,000-codepoint extension-B range.

That shortcut is only safe when the face really does cover the whole range.
Noto CJK has no U+2A6A5; a block over extension B would claim it, `Face::has`
would answer yes, the rescue face would never be consulted and the Character
would draw as `.notdef` (ADR-0039).

The previous generator resolved this by emitting blocks only for faces with a
`glyf` table, where the per-glyph ink list is the authoritative coverage list and
the block is only a width. A CFF face therefore fell back to one entry per
codepoint: 32,500 rows, four faces, **1.1 MB** for the embedded table.

ADR-0073 then gave CFF faces punctuation ink, which made the 1.1 MB unavoidable
unless blocks came back.

## Decision

A face without `glyf` emits **one block per run of consecutive covered
codepoints that share the mode advance**. A hole ends the run instead of hiding
inside it:

```text
covered:  4E00 4E01 4E02 ··· 4E0F      (gap)      4E11 ···
block:    [4E00..4E0F] adv=4096                   [4E11..4E1F] adv=4096
```

Then a block is exact in both directions: every codepoint it names exists and has
the stated advance, and every codepoint it does not name is an explicit entry or
absent. `Face::has` stays correct without a per-glyph list.

The 90% mode threshold is kept: a block range whose advances are not mostly
uniform emits plain entries, because a run-based block would then be one block
per codepoint.

## Consequences

The embedded table (`Noto Serif/Sans CJK SC` regular and bold, plus Noto Sans
Math) goes from a version 1 blob of 84,720 bytes with no ink and no math data to a
version 5 blob of **287,898 bytes** with:

| section | before (v1) | after (v5) |
|---|---|---|
| advances | 6 coarse blocks + few entries | 1,793–1,846 run blocks + 1,962 entries per face |
| punctuation ink | none | 2,686–2,744 marks per face |
| `MATH` constants + glyph ink | none | 35 constants, 2,901 glyphs (math face) |

Coverage is now exact where the old blob over-claimed — the test
`embedded_table_never_claims_a_codepoint_it_lacks` pins U+2A6A5 as absent and
U+4E00 as present on all four CJK faces.

The layout had to be re-baselined, because the embedded table now supplies ink
that it never had:

| metric | before | after |
|---|---|---|
| pages / paragraphs / lines | 48 / 599 / 1,213 | **identical** |
| `fitness_jumps` | 3 | **8** |
| `jitter_p95` | 377‰ | **377‰** |
| `over_compress`, `emergency`, widows, orphans | 0 | 0 |

No break moved. The jumps rose because a line ending on a full-width mark now
gives back its real side bearing (0.82 em for `，`) instead of the 0.5 em
constant, which moves those lines further into the stretch range and flips a few
adjacent pairs across a fitness bucket. The finer-grained jitter is unchanged.
`crates/soma-eval/tests/corpus.rs` records the new floor and why.

## Alternatives

- **Keep one entry per codepoint.** 1.1 MB, and the table would still need the
  ink section on top. Rejected.
- **Add a `holes` section to the format.** Exact and compact, but it is a schema
  version bump for a problem that run-splitting solves inside the existing
  format. Rejected.
- **Drop blocks and rely on `default_advance`.** CJK is mostly uniform, but a
  proportional Latin run would silently get the wrong width. Rejected.
- **Ship no ink in the embedded table** (D96 as recorded before this ADR). Keeps
  the table small, keeps the comma notch. Rejected once the user reported it.

## Verification

```sh
cargo run -p soma-eval -- corpus        # the table above
cargo test --workspace                  # 782 tests
bash scripts/gates.sh                   # 12 gates, ALL,48,599,1213,0,0,3,0,0,8,486,1580,377,9
cargo test --manifest-path tools/metrics-gen/Cargo.toml   # 4 tests
```
