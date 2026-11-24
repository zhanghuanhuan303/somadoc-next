# ADR-0087: Natural-width tables and dash-derived column shares

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0065 (column shares), ADR-0081 (reference template),
  ADR-0084 (table rule separation), `docs/template.md`
- Touches: `soma-template` (`TableSpec.width`), `soma-layout` (table sizing),
  `soma-md` (GFM delimiter dashes), `examples/templates/plex.yaml`,
  `crates/soma-template/tests/a2_template.rs`,
  `crates/soma-md/tests/a5_md.rs`,
  `crates/soma-layout/tests/a6_blocks.rs`, `docs/capabilities.md`

## Context

The pandoc reference renders a short three-column table at its content width
and centres it. The engine always filled the measure: `column_widths` scaled
the measured columns until their sum equalled the content width. The result
was a correct but visually different table: very wide columns with large gaps.

The Markdown frontend also dropped information that GFM/pandoc users expect:
the number of dashes in the delimiter row only selected alignment, not the
relative column widths.

A latent measurement bug surfaced while implementing natural sizing:
`natural_width` summed `boundary.glue.width`, but a space boundary stores
`Glue.width = 0`; the kernel asks the metrics for the actual space. A natural
column was therefore short by one space per gap and wrapped a line it had just
measured as fitting.

## Decision

### Template width mode

`table:` gains `width: justify | natural`, default `justify`:

- **justify** keeps the historical behaviour: columns share the whole measure.
- **natural** gives the table its content width. Each column contributes its
  measured width plus `0.5em` of horizontal cell padding on both sides (the
  analogue of `\tabcolsep`). The table is centred in the measure. If the total
  reaches or exceeds the measure, it falls back to the justified sizing, so an
  oversized table still fits the text block.

The canonical template writes `"width":"natural"` only when selected; the
default keeps the existing template hash bytes and the schema id stays
`soma.template/9`.

### Markdown dashes

`table_delimiter` returns the alignment and a per-mille share for each column,
derived from the dash counts. The shares sum to 1000 with largest-remainder
rounding (left to right on ties). `Column.share` carries them; DocIR still has
no physical widths.

- Under **justify**, the shares are per-mille of the measure, as for an HTML
  `<colgroup>`.
- Under **natural**, the shares can widen a column beyond its measured
  content; more dashes means more of the table's natural total. A column is
  never narrowed below its measured width (including the two half-em pads),
  because that overwrites the padding and produces a one-character strip. The
  table grows to fit and is centred while it remains below the measure; if it
  reaches the measure it falls back to justified sizing.

### Measurement

`natural_width` now asks `metrics.space_width` for `BoundaryKind::Space`, so a
cell containing spaces measures the same width the breaker will use.

## Consequences

- The Plex reference template selects `width: natural`; its table is centred
  and compact, like the pandoc page.
- A Markdown table `| --- | -------- |` gives shares `272 / 728`; equal dashes
  are the ordinary GFM spelling and carry no preference, so the columns keep
  their measured widths. More dashes can widen the later column and therefore
  the table, but a short column never collapses into its padding.
- Justified tables keep filling the measure and keep their existing page
  geometry; the space-width fix can shift column proportions slightly, but the
  table-page fixture stays at 2 pages / 125 lines.
- Canonical template/style hashes change only for a template that selects
  `natural`; the sample's template hash changes as expected.
- Three renderers need no new code: the table positions come from layout.

## Alternatives

- **Make natural the global default.** It would change every existing table
  and page history; rejected. A template opts in.
- **Put the flag on DocIR's `Table`.** It would let a document override its
  template, but width is presentation (ADR-0009); rejected.
- **Treat equal dash counts as equal columns.** It would turn every ordinary
  `--- | ---` table into equal columns and lose the content-width behaviour;
  rejected. Equal counts mean "no preference".
- **Let natural columns shrink below their measured width.** Text would run
  into the padding and the neighbouring cell, producing vertical characters;
  rejected. Dashes widen, never squeeze below content.

## Alignment

- `crates/soma-template/src/template.rs` — `TableWidth`, `TableSpec.width`,
  parser.
- `crates/soma-template/src/canonical.rs` — conditional width key.
- `crates/soma-template/src/style.rs` — `StyledDoc::table_width`.
- `crates/soma-layout/src/lib.rs` — natural sizing, cell padding, space-aware
  natural width.
- `crates/soma-md/src/lib.rs` — dash shares.
- `examples/templates/plex.yaml` — `width: natural`.
- `docs/template.md`, `docs/capabilities.md` — user-facing description.
