# ADR-0010: Borrow-first layer boundaries and optional parallelism

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §4, §11, §15.1, §18; ADR-0001, ADR-0007,
  ADR-0009

## Context

Architecture v2 fixes the layer contracts and the determinism gates, but leaves
two implementation-shape questions open. Both become expensive if deferred:

1. **Do layers deep-copy their inputs?** If the MVP clones `Document` into the
   box tree, then adding borrows or `Arc` later changes `ParagraphBox`,
   `LayoutResult`, and every A3–A5 signature, which invalidates goldens and the
   hash gates with it.
2. **Where does parallelism enter?** A choice made after the fact tends to leak
   threads into the core and to make output depend on scheduling, which breaks
   the native/wasm byte-identity gate.

Two facts constrain the answer:

- `soma-core` and `soma-doc` are `no_std` + `alloc`; `rayon` requires `std` and
  real threads.
- The inline result (`soma.layout/6`) deliberately contains no text, only
  indices, `StyleId`, and integers. A renderer therefore needs a *text source*
  in addition to the result.

## Decision

### 1. Page-level result is owned data and records its source node

`soma-layout` produces a page-level result (working name `PagedLayout`) with its
own schema id. It owns all of its data — no lifetime, no reference into
`Document`. Every line records the `NodeId` of the DocIR block it came from, so
renderers, incremental relayout, and per-block quality metrics can resolve text
without holding the document.

### 2. Renderers receive a narrow text seam, not the document

Rendering takes the result, the style table, and a trait object:

```rust
pub trait TextSource {
    /// Text of one atom inside a DocIR block.
    fn atom_text(&self, block: NodeId, atom: u32) -> &str;
}

pub fn render(result: &PagedLayout, styles: &StyleTable, text: &dyn TextSource) -> String;
```

`Document` may implement `TextSource`; a cached extraction may implement it too.
A renderer must not take `&Document`, and must not re-break or re-paginate.

### 3. `StyledDoc` is pure data with no lifetime

Style resolution is a data product, not a borrowing wrapper:

```rust
pub struct StyledDoc {
    /// Sorted by `NodeId`; the sort is part of the canonical form.
    node_style: Vec<(NodeId, StyleId)>,
    table: StyleTable,
}

pub fn resolve(doc: &Document, template: &Template) -> Result<StyledDoc, StyleError>;
pub fn layout(
    doc: &Document,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
    engine: &EngineConfig,
) -> Result<PagedLayout, LayoutError>;
```

Consequences: `StyledDoc` can be cached, hashed, and sent across threads;
`styleHash` becomes available without a new mechanism.

### 4. Atom materialization is expected and measured, not "zero-copy"

`Document` stores text in `String`s; the frozen `Atom` also owns a `String`. The
conversion `DocIR -> ParaIR` therefore allocates once per atom. This is accepted:

- layer boundaries must not deep-copy the document or the box tree;
- the single `DocIR -> Atom` materialization is allowed and is the main
  allocation source to watch;
- changing `Atom` to borrow or to inline small strings is out of scope here and
  would require a kernel ADR (ADR-0007) plus a model-version bump.

A3 must measure allocations for a 4,000-atom paragraph before any such change is
proposed.

### 5. Parallelism lives outside the core, behind its own crate

`soma-layout` stays `no_std` + `alloc` and always serial. Parallel orchestration
lives in a separate `std` crate (working name `soma-layout-par`) that calls the
serial API. Consequences:

- the serial path is the only hash baseline;
- "disable parallelism" means "do not depend on that crate";
- wasm builds never enable it, by construction rather than by a runtime flag;
- the core gains no `feature = "parallel"` branches.

Parallel axes, in cost order:

1. **paragraph breaking** across independent `ParagraphBox`es (dominant cost);
2. page box assembly after breakpoints are fixed (pagination itself is serial:
   vertical K-P needs paragraph heights);
3. within one footnote-iteration pass; the iteration loop stays serial.

Never parallel: the K-P DP inside one paragraph; template resolution and DocIR
construction in the first version.

### 6. Determinism and performance gates

- `Metrics` is a pure function of `(StyleId, text)`; caches may memoize it and
  must not change output. The trait is declared `Metrics: Sync` so
  `&dyn Metrics` can be shared.
- Parallel and serial paths must produce byte-identical canonical results, for
  `RAYON_NUM_THREADS` in `{1, 2, 8}`.
- Output must not depend on thread count or scheduling; parallel collection must
  be order-preserving (indexed collect), never "push into a shared vector".
- Degenerate sizes are serial: below a page-count threshold, and whenever
  `available_parallelism() == 1`, the parallel path must be skipped, not panic.
- A performance gate records, per run: atom allocation count, peak RSS, and
  P50/P95 wall time. Wall time is reported, allocation count and canonical bytes
  are asserted.

### 7. FFI boundary

`Arc` does not cross `wasm32-wasip1` / `wasm-bindgen` directly. The exported
boundary uses either a handle table (`u32` -> slab entry) or canonical JSON in /
JSON out. Rust ownership rules must not leak into the host.

## Consequences

- A3–A5 signatures are decided before their first line: no `clone` of `Document`
  into the box tree, no lifetime on `StyledDoc`, renderer takes a `TextSource`.
- `styleHash` (debt D30) becomes mechanical: canonicalize `StyledDoc`.
- The serial path stays the single regression oracle; parallelism cannot affect
  what is measured.
- The core stays `no_std`, and wasm determinism does not depend on a flag being
  off.
- The largest remaining allocation cost (`Atom.text`) is visible in the gate
  instead of being hidden by the word "zero-copy".

## Alternatives rejected

- **Clone at every layer, optimize later.** Changes box-tree and result
  lifetimes after goldens exist; forces A3–A5 rework and hash churn.
- **`LayoutResult` carries text.** Roughly doubles result size and duplicates
  the document; also makes the inline schema heavier than the frozen
  `soma.layout/6`.
- **Renderer receives `&Document`.** Leaks the whole semantic tree into the
  render layer, breaks ADR-0009's boundary, and forces the FFI to ship two
  objects.
- **`StyledDoc<'a>` borrowing the document.** Prevents caching, hashing, and
  cross-thread reuse for no benefit; the data is already a flat table.
- **`feature = "parallel"` inside `soma-layout`.** Pulls `std` into a `no_std`
  layer and makes "is parallelism off?" a runtime question.
- **Parallelize the DP.** Single-paragraph line breaking is serially dependent;
  parallelizing it changes tie-break order and canonical bytes.
