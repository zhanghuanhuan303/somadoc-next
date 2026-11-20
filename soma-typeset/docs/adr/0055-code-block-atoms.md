# ADR-0055: a code block owns one atom table, and a code line keeps its spaces

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0011 (inline span compiler), ADR-0024 (code blocks),
  `docs/debt.md` D70
- Touches: `soma-rules` (new entry point), `soma-layout` (`code_item`).
  No schema change.

## Context

A four-line `verbatim` block printed as three lines of rubble: `soma render
--input song.tex` came out correctly, and every line after it came out as
`smder - ispng`, characters dropped and others in the wrong place.

Two independent bugs lined up:

1. `code_item` compiles each source line with `paragraph_from_text`, which
   returns a paragraph whose line ranges are *local to that paragraph*. Each
   broken line was then stored with `start = 0`, but the block concatenates all
   lines' atoms into one table. Every line after the first read the first
   line's atoms, and the renderer, which looks up `(node, atom)`, drew the first
   line's glyphs at the other lines' advances.
2. `paragraph_from_text` collapses whitespace by design (that is what prose
   wants). A code line's leading indent and its argument spacing are content,
   so the block lost its indentation and `--template song.yaml` became
   `--template song.yaml` only by luck of the spacing between words.

Neither was caught because every fixture used a one-line code block.

## Decision

### 1. The block owns the atom table; each line is offset onto it

`code_item` records `base = atoms.len()` before extending and adds `base` to
`line.start`, `line.end` and every `box.atom`. Line ranges are block-absolute,
the same convention paragraphs already use.

### 2. A code line compiles with `paragraph_from_code_line`

A new entry point in `soma-rules`:

```rust
pub fn paragraph_from_code_line(text, style, style_id) -> Result<Paragraph, RuleError>
```

It pushes one atom per whitespace character and compiles the runs between them
with the normal rules. Spaces stay spaces, a leading run is not dropped,
`finish` still reports `EmptyText` for a blank line — which is how `code_item`
knows to emit an empty line box.

The line is still not wrapped: `code_item` breaks each line against a 1000 em
measure, so a long line overflows rather than reflowing.

## Consequences

- `examples/book/song.tex` prints its command block as four readable lines,
  with the continuation indent intact.
- The atom table is now a stable identity: `atom_text(node, atom)` is
  unambiguous across the whole block, which the HTML and SVG renderers rely on
  as much as the PDF one.
- Two new tests: the second line of a two-line block resolves to its own text
  and does not overlap the first line's atoms; a code line keeps `"  a b"` as
  `[" ", " ", "a", " ", "b"]`.

## Alternatives

- **Give the code block one paragraph per line with a `line_offset` field.**
  That is a schema change on every renderer for a bug that a single `+= base`
  fixes, and it would make `atom_text` ambiguous again.
- **Keep `paragraph_from_text` and pad with indent boxes.**
  It hides the reason indentation disappeared and still collapses the interior
  runs. Code is not prose; it needs its own compiler entry point.
