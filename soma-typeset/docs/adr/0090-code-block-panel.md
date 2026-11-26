# ADR-0090: Code block panels use background and padding from the template

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0091 (syntax palette), ADR-0089 (monospace family),
  ADR-0088 (Plex default), ADR-0081 (Plex reference)
- Touches: `soma-template` (`background`, `padding-*`), `soma-layout`
  (`code_item`), `soma-render-pdf`, `soma-render-html`, `soma-render-svg`,
  `examples/templates/plex.yaml`, `docs/template.md`,
  `crates/soma-template/tests/a2_template.rs`,
  `crates/soma-layout/tests/a6_blocks.rs`

## Context

The Rust book draws a code block as a light panel: `#f6f7f6` background,
`1rem` of padding on all sides, no border, and code text inset from the panel
edge. The panel fills the measure; the code itself never wraps. This is not
decoration around a block — the padding changes where the first glyph starts
and how much vertical space the block consumes, so it must be resolved before
pagination, not added by each renderer independently.

The template model had no paint properties at all. Adding a renderer-only
`codeBackground` would duplicate geometry in three backends and let the PDF
and HTML disagree about the block height.

## Decision

### Style properties

`ComputedStyle` gains five non-inherited properties, resolved with the same
line-height/font-size rules as every other length:

- `background: <color>` — a filled rectangle behind the block. Default none.
- `padding-top`, `padding-right`, `padding-bottom`, `padding-left: <length>` —
  the inset between the panel edge and the block's content. Default zero.

They are ordinary template declarations, so they can apply to any element; the
first user is `codeBlock`. A later block quote or admonition panel can use the
same fields without a new layout concept.

The canonical style JSON writes each field only when it differs from the
default. A template that does not use panels therefore keeps its historical
style hash bytes; the schema id stays `soma.styles/7`.

### Layout

`code_item` builds the panel, not the renderers:

- `panel width = measure` (a code block is full-measure, like the reference).
- `panel height = padding-top + sum(line heights) + padding-bottom`.
- every line's `y` starts at `padding-top`;
- every atom's `x` is shifted right by `padding-left`;
- the code line still breaks with a 1000 em target, so long lines overflow
  (copyable commands are never broken mid-token), exactly as before.

The block is already atomic (`parts` empty), so a panel never splits across
pages and the top/bottom padding cannot be lost at a page break.

### Renderers

Each backend fills `block.x/block.y/block.width/block.height` with
`background`, then draws the atoms at their existing positions. No renderer
adds padding of its own, and no renderer needs to reinterpret
`padding-left` — it is already in the atom coordinates.

- PDF: a filled rectangle before the tagged text run.
- SVG: a `<rect>` before the block's `<text>`.
- HTML: `background` on the block's absolutely positioned `<div>`.

## Consequences

- The Rust book panel is deterministic and identical across backends because
  there is one geometry source.
- `space-before`/`space-after` remain outside the panel; they are flow glue,
  not padding.
- A code block taller than the page still overflows as one atomic item; a
  panel cannot be split. This matches the previous atomic behaviour.
- Code text begins at the padding edge, so the first column of a panel is not
  flush with body text. That is the reference's visual cue.
- The Plex template sets `background: "#f6f7f6"` and `0.8em` horizontal /
  `0.5em` vertical padding. The exact numbers are template choices, not engine
  constants.

## Alternatives

- **Paint the panel in each renderer from template values.** Geometry would be
  recomputed three times and pagination would not see the padding; rejected.
- **Model padding as `indent-left`/`indent-right`.** It would also indent the
  panel edge or require a "panel width" concept; the background would not know
  where to start. Rejected.
- **Add a border-left accent.** The Rust book panel has no border; adding one
  now would be a second design. The fields can be extended later.
- **Wrap long code lines.** It would break copyable commands and change the
  layout of every existing code block; rejected.

## Alignment

- `crates/soma-template/src/template.rs` — property ids and parsing.
- `crates/soma-template/src/style.rs` — `ComputedStyle` fields and `apply`.
- `crates/soma-template/src/canonical.rs` — conditional fields.
- `crates/soma-layout/src/lib.rs` — `code_item` panel geometry.
- `crates/soma-render-pdf/src/lib.rs`, `soma-render-html`, `soma-render-svg` —
  background fill.
- `examples/templates/plex.yaml` — Rust book values.
- `docs/template.md` — property reference.
