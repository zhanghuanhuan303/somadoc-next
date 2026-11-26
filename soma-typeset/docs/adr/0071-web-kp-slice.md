# ADR-0071: the browser slice is `soma-core` + `soma-rules` behind a C ABI

- Status: Accepted
- Date: 2026-11-21
- Related: ADR-0007 (the kernel is frozen), ADR-0010 (zero-copy and optional
  parallelism), ADR-0072 (the emergency window), D94, D95
- Touches: `soma-core` (`Metrics` gains three defaulted methods),
  `soma-web` (new crate), `web/` (JavaScript), `scripts/web.sh`

## Context

The engine can already lay out a PDF. A browser preview has a different
constraint set: it must reflow while the user types, it cannot ship a font
parser, and it must not grow a second implementation of the Chinese rules — the
whole point of the project is that the same cost model decides every break.

Three questions had to be answered before writing code:

1. **What is the smallest slice that still decides breaks the same way?**
   Measurement showed `soma-core` + `soma-rules` compiles to 48,944 bytes raw,
   21,891 bytes gzip, with `WebAssembly.Module.imports() == []`. The rules layer
   costs about 3 KB gzip. Rewriting the rules in JavaScript to save 3 KB would
   be the most expensive decision in the project.
2. **Who measures the text?** Only the browser knows how it will draw the
   glyphs. Shipping `.smt` tables instead would make the preview disagree with
   the browser's own rendering (D95).
3. **What crosses the boundary?** The kernel only ever asks *how wide is this
   atom*. That answer is already an integer; the text itself never needs to
   cross back.

The blocking problem was the seam: `Metrics::advance(&self, text, style)` is
keyed by text, while a host that measured the atoms holds an *array indexed by
position*. Working around it with a call-order cursor is unsound — `Shaped::new`
asks once per atom in order, but `line_vertical` asks again after the break is
chosen, and a `Hyphen` boundary inserts an extra `advance("-")` in between.

## Decision

### 1. The slice is rules + K-P, nothing else

`soma-web` depends on `soma-core` and `soma-rules` only. Layout (boxes, pages,
footnotes), templates, the `.smt` metrics parser, and every renderer stay out.

### 2. The kernel gains three defaulted `Metrics` methods

```rust
fn atom_advance(&self, index: u32, atom: &Atom, style: StyleId) -> Width;
fn atom_glyph_metrics(&self, index: u32, atom: &Atom, style: StyleId) -> GlyphMetrics;
fn hyphen_width(&self, style: StyleId) -> Width;
```

The defaults reproduce the current dispatch exactly (`object_advance` for
`InlineObject`, `advance` otherwise), so every existing adapter — synthetic,
table-backed, math — is byte-identical. This is the "default 0" form of
extension that ADR-0007 allows.

The index is the point: a host that already measured the atoms answers from its
own array, and the answer does not depend on call order.

### 3. Measurement stays in the host

The host measures each atom with canvas `measureText`, quantises to 1/4096 em,
and passes one `i32` per atom. `soma_break` therefore takes a width array, not
text.

Measured cost on this machine (Chromium): 660 CJK atoms in 0.30 ms, 0.45 µs per
atom. The first `measureText` after a font load costs 3.3 ms, which is why the
demo waits for `document.fonts.ready` and re-lays out once.

### 4. The boundary carries integers only

| Buffer | Layout |
|---|---|
| style | 7 words: width, firstIndent, hang, tracking, lineHeight, styleId, flags |
| spans | 5 words each: byte offset, byte length, styleId \| kind << 8, decor, reserved |
| text | one UTF-8 blob the span offsets point into |
| break input | 8 header words + one advance per atom |
| break output | 4 header words + 9 per line + 3 per box |

Boundaries are **not** exported. The host does not need to know that a break was
forbidden, trimmed, or penalised; it needs line ranges and box coordinates.
Box coordinates come from the module, so the browser and the PDF/SVG renderers
consume the same numbers — "the renderer never re-breaks" holds on the web too.

### 5. No `wasm-bindgen`

The module has no imports and no glue. `new WebAssembly.Instance(module, {})`
works in Chromium and in node. The 120-line ES module in `web/soma-kp.js` is the
whole adapter. `wasm-bindgen` would add a dependency, a build step, and string
marshalling on a path that has no strings.

### 6. Deploy as plain files

`web/` has no `package.json` build step (only `"type": "module"`), no bundler,
no CDN. Four files: the module, the ES module, the worker, and the demo.

## Consequences

- The browser and the PDF share one rules layer and one DP. A change to either
  shows up in `web/parity.txt`, which the Rust ABI test and the Chromium harness
  both read; `scripts/web.sh` fails if the fixture is stale.
- The slice is 32.7 KB gzip, which leaves room for a `.smt` parser later without
  changing the ABI.
- Preview breaks are guaranteed to match *the browser's own painting*, not the
  PDF's `.smt` widths (D95). Hosts that need the PDF's breaks can pass their own
  width table today; a faithful mode is a follow-up.
- The three methods widen `Metrics`. Any future host that can answer by position
  — a server-side pre-measure, a glyph cache — gets the same seam for free.

## Alternatives

- **Rewrite the rules in JavaScript.** Saves 3 KB, duplicates the punctuation
  table, and guarantees the two engines drift. Rejected.
- **Ship the `.smt` table into wasm and measure there.** Makes the preview
  disagree with what the browser paints, and adds a megabytes-scale binary or a
  character-set subsetting tool. Deferred as `faithful` mode.
- **Cursor-based `Metrics` adapter.** No kernel change, but unsound: the
  post-break `line_vertical` pass and the hyphen query both desynchronise it.
  Rejected after it failed in the measurement spike.
- **`wasm-bindgen` for ergonomics.** Rejected: nothing at this boundary needs
  strings or structs.

## Verification

`bash scripts/web.sh` — W1 zero imports, W2 size, W3 parity with the native
fixture, W4 timing, W5 memory stability, W6 the fallback policy.
