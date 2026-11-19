# ADR-0013: Line-level vertical layout

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0005 (pagination), ADR-0006 (determinism), `docs/proposal-effect-max.md` E2
- Supersedes: the "vertical flow atom is a block" part of ADR-0005. Greedy stays.

## Context

ADR-0005 modelled the vertical flow as one item per block. A paragraph was
therefore indivisible: if it was taller than the content box it either over
flowed or was pushed whole to the next page, leaving a hole. Widow/orphan rules
had nowhere to attach, and footnotes could only move with their block.

Vertical heights, unlike horizontal widths, are fixed and additive, so a page
breaker can reason about them exactly.

## Decision

The vertical atom is one **line**, not one block.

```rust
struct VItem {
    item: usize,   // FlowItem
    part: usize,   // line within that item
    height: Width,
    before: Glue { width, plus, minus },
    after: Glue,
    notes: Vec<usize>,
    penalty: i64,      // club / widow, charged when breaking after this line
    can_break: bool,   // false for keep-with-next
}
```

- Paragraphs emit one `VItem` per line; atomic blocks emit one `VItem`.
- Forced page breaks and `page-break-before` split the stream into **segments**
  first; each segment runs its own DP. This mirrors the horizontal forced-break
  design and keeps "must break" out of the cost function.
- Page fill follows TeX: glue at the top of a page is discarded; glue after the
  last item counts toward the fill; stretch and shrink on that glue feed the
  page badness.
- Page badness normalizes leftover space by the **body line height**, not by the
  page height: one line short is 1000 permille, not 25.
- `club_penalty` (break after a paragraph's first line) and `widow_penalty`
  (break before its last line) are finite. A page can still break there when the
  alternative is worse.
- Footnotes are charged to the page that holds the **marker line**; a prefix sum
  over line-attached notes makes the span cost O(1).
- Stretchable glue (`space-before-plus`, `space-after-plus`) is distributed into
  the page's free space, so block positions reflect the stretch.
- Because heights are monotone, "stop when the page is full" is an **exact**
  prune. Cost is O(items × lines-per-page), not O(n²) in document size.

## Consequences

- `PlacedBlock` gains `line_offset` and `lines_total`; the same `NodeId` may
  appear on two pages, and `(node, line_offset)` is unique.
- `Mark` gains `line`, so a marker travels with its line across a page break.
- `Page.header`/`footer` exist from this point (see ADR-0015).
- `soma.pages/2` then `/3`. Greedy remains as an explicit strategy and as the
  fallback when the DP finds no feasible break sequence.
- `VerticalParams` gains `club_penalty`/`widow_penalty` and is hashed into
  `verticalHash`, so two pagination models cannot share canonical bytes.

## Alternatives rejected

- **Split paragraphs greedily at the page boundary.** Cannot trade off, and
  produces ragged page bottoms.
- **Keep block-level DP and add a fix-up pass.** Widows/orphans become a repair
  heuristic instead of part of the optimum.
- **Infinite penalties for widows/orphans.** Makes some page fills infeasible
  and forces a fallback even when a slightly worse-looking break exists.
