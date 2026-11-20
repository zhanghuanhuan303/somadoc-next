# ADR-0045: the deflate encoder builds a dynamic Huffman table when it pays

- Status: Accepted
- Date: 2026-11-20
- Supersedes: the "fixed Huffman only" part of ADR-0043
- Related: ADR-0006 (determinism), ADR-0034 (PDF backend), `docs/debt.md` D82
- Touches: `soma-render-pdf` only. No schema, no layout hash.

## Context

ADR-0043 compressed every PDF stream with one fixed-Huffman deflate block. It
was deterministic and simple, but the fixed table spends 8–9 bits on a literal
that a per-document table could spend 3 bits on. A font program is exactly that
case: a handful of bytes (`0x00`, the high byte of most glyph indices, a few
common deltas) make up most of the stream, and the fixed table charges them
full price.

ADR-0043 recorded the trade-off and deferred dynamic Huffman as "perhaps
another 5–10%". Measurement said otherwise on the CJK samples, so this ADR
finishes the job.

## Decision

### 1. Encode both, keep the shorter

`zlib()` tokenizes once (greedy LZ77, unchanged from ADR-0043), then encodes
the token stream twice:

* `encode_fixed` — `BTYPE=01`, the RFC 1951 §3.2.6 table;
* `encode_dynamic` — `BTYPE=10`, a table built from the block's own
  frequencies, including the code-length code.

The shorter body wins; ties go to the fixed table, which has no header to get
wrong. Comparing the two finished encodings is cheaper and more honest than
modelling when a dynamic header pays for itself, and it stays deterministic
because the comparison is on byte lengths.

### 2. The tree shape is a function of the counts and nothing else

Frequencies are counts, so the tree depends only on the input bytes. Leaves are
merged smallest-first with the lower node index as the tie-break; there is no
comparator that could disagree between targets, and no hash-map iteration.
Canonical codes follow RFC 1951 §3.2.2 (sorted by length, then symbol).

### 3. Over-deep trees fall back to a balanced code, not to an overflow fix-up

A code longer than 15 bits is invalid. The textbook repair (zlib's `gen_bitlen`
overflow loop) redistributes depth counts with an approximate Kraft argument;
reproducing it correctly is subtle, and an *incomplete* code makes a reader
reject the file, which is a bad failure mode for a PDF backend.

Instead, when the natural tree is deeper than the limit the alphabet switches
to the two-level balanced code: `n` symbols get `floor(log2 n)` or
`ceil(log2 n)`
bits with the Kraft sum exactly one, short codes to the most frequent symbols.
For the alphabets in play the balanced code always fits the limit (286 symbols
→ 9 bits against a 15-bit cap; 19 code-length symbols → 5 bits against 7), so
the fallback is always legal. It costs a little optimality in a case that does
not occur in practice — but it is provable by inspection and there is a unit
test that builds a Fibonacci-shaped alphabet, checks the natural tree really
does overflow, and checks the fallback's Kraft sum is exactly `1 << 15`.

### 4. The test decoder covers both block types

`deflate.rs` keeps its independent decoder, extended with a dynamic-table
reader: `HLIT`/`HDIST`/`HCLEN`, the permuted code-length alphabet, the
`16/17/18` repeat codes, and canonical lookups through a
`(length, code) → symbol` table built from the decoded lengths. Round trips
cover repetitive data, long-distance text, every byte value, a skewed binary
alphabet (a font-shaped distribution) and determinism. A separate test asserts
the dynamic block is actually chosen when it should be.

## Consequences

Same input, same layout, measured on this machine by running the encoder
both ways over the same layout:

| sample | fixed only (ADR-0043) | dynamic (this ADR) |
| --- | --- | --- |
| `anthology.md`, MiSans + rescue, 49 pages | 537,163 B | **457,559 B** (−14.8%) |
| `two-font.md`, Noto Serif CJK, 8 pages (CFF) | 1,629,365 B | **1,581,063 B** (−3.0%) |

The CFF sample gains least: its bulk is a private-dict and subroutine blob that
is already close to incompressible bytecode. ADR-0043's own snapshot of the
same sample was 1,627,922 B for the fixed encoder; the small drift comes from
the metrics table, not from compression.

- The encoder grows by roughly 300 lines plus the test decoder; the crate is
  still dependency-free, `no_std` and deterministic.
- `stream_flate` is unchanged: it asks `deflate::zlib` for the best stream and
  falls back to raw bytes when compression does not gain at least 16 bytes.
- Compression is still not part of `canonicalHash` (ADR-0006): the hash covers
  the layout, not the PDF bytes.

## Alternatives

- **A third-party deflate (`miniz_oxide`, `flate2`).** Deterministic in
  practice, but it would be the crate's first dependency and would need
  `no_std` + `wasm32` verification forever. The contract is byte-identical
  output; owning the encoder is how that stays true.
- **Dynamic only.** Loses on small `/ToUnicode` maps and short content
  streams, where the code-length header costs more than it saves. Encoding both
  and comparing is cheaper than predicting.
- **Stored blocks for incompressible data.** Already covered: `zlib` returns
  `None` and the caller stores the bytes raw, which is exactly a stored block
  without the block header.
