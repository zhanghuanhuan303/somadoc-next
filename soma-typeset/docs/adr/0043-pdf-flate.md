# ADR-0043: PDF streams are deflated by a fixed-Huffman encoder

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0034 (PDF backend), ADR-0039 (CFF), ADR-0006 (determinism),
  `docs/debt.md` D82
- Touches: `soma-render-pdf` only. No schema, no hash of the layout changes.

## Context

Every PDF the backend writes stored its streams raw: the font program, the
page content, the `/ToUnicode` CMap. A 49-page anthology was 1.69 MB, an
eight-page sample with a CJK CFF font was 1.92 MB. D82 listed "no stream
compression" as a known gap.

PDF's `/FlateDecode` is the obvious answer, but the backend's contract is
byte-identical output for a given layout (ADR-0006), so the compressor has to
be deterministic. A third-party deflate would also be the first dependency of
a crate that has none, and would have to build for `no_std` and for
`wasm32-unknown-unknown`.

## Decision

### 1. A hand-written fixed-Huffman deflate

`src/deflate.rs` emits one deflate block per stream:

* `BFINAL=1`, `BTYPE=01` — the fixed literal/length table of RFC 1951 §3.2.6,
  which needs no header and no code-length encoding;
* greedy LZ77 with a 32 KiB window, a 15-bit hash of the next three bytes, and
  a chain limit of 48 candidates — bounded work, no allocation per match;
* the zlib wrapper (`0x78 0x01`) and an Adler-32 trailer.

Nothing about it depends on iteration order, time, or floating point, so two
runs of the same input produce the same bytes. That is the whole reason it is
hand-written instead of imported.

### 2. Only smaller streams are compressed

`stream_flate` compresses and falls back to the raw bytes when the result is
not at least 16 bytes smaller. Streams carry `/Filter /FlateDecode` only when
they are compressed, so a short `/ToUnicode` CMap stays readable and the
output never grows.

Applied to: the font program (`/FontFile2`, `/FontFile3`), every page content
stream, and the `/ToUnicode` CMap. Dictionary objects are not streams and are
unchanged.

### 3. The round trip is tested, not assumed

The unit tests carry a decoder for the one block type the encoder emits — an
independent implementation of the code tables in `deflate.rs` — and round-trip
repetitive data, text with long-distance matches, every byte value, and a
determinism check. `cargo test` therefore fails if a length code, a distance
code or the bit order is wrong, without needing a PDF reader in the test
suite. Poppler (Gate R and the CLI samples) then checks the real consumer.

## Consequences

Measured on this machine, same input, same layout:

| sample | raw | deflated |
| --- | --- | --- |
| `two-font.md` with Noto Serif CJK (8 pages, CFF) | 1,919,666 B | **1,627,922 B** (−15%) |
| `anthology.md` with MiSans + rescue (49 pages) | 1,686,314 B | **527,273 B** (−69%) |

The CFF sample gains least because its bulk is a private-dict and subroutine
blob that is already close to incompressible bytecode; the page-heavy sample
gains most because CJK content streams are long runs of two-byte codes and
operators.

- D82 keeps "no tagged PDF, no images, no annotations" but loses "no stream
  compression".
- A dynamic-Huffman block would buy perhaps another 5–10% on the font blob and
  cost a code-length encoder, a frequency pass and a second decoder to test.
  Not now; the fixed table already removes the bulk of the redundancy, and the
  encoder's determinism is easy to see.
- Compression is not part of the canonical result: `canonicalHash` covers the
  layout, not the PDF bytes. Two builds that compress differently still agree
  on the layout.
