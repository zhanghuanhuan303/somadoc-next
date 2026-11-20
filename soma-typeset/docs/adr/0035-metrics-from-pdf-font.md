# ADR-0035: Layout metrics can come from the same font the PDF embeds

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0006 (determinism), ADR-0034 (PDF backend), `docs/precision.md`,
  `docs/cli.md`
- Touches: `soma-cli` (`--metrics`), `scripts/book.sh`,
  `crates/soma-cli/tests/cli.rs`

## Context

The PDF backend embeds a TrueType font chosen by the caller (MiSans in the
example) while layout used the committed Noto metrics table. For CJK the two
agree exactly (every Han glyph is 1em), but Latin runs, digits and some
punctuation can differ by a few percent. The precision experiment in
`docs/precision.md` showed that 1/4096 quantization is negligible; the larger
error source is **measuring with one face and drawing with another**.

`soma-metrics` already parses a committed `soma.metrics/1` table at runtime, and
`tools/metrics-gen` already builds such a table from a TTF/TTC face. The missing
piece was a CLI path for the caller to supply that table.

## Decision

`soma render` and `soma check` accept:

```text
--metrics <file>     soma.metrics/1 table; omitted uses the embedded Noto table
```

`load_metrics` reads the file and calls `MetricsTable::parse`; an invalid or
missing file is a CLI error. The parsed table is passed to the existing
`StyledMetrics` adapter, so layout, canonical `metricsHash`, and every renderer
see the same widths. No core code learns to open fonts.

`scripts/book.sh` generates the table from the PDF font when it writes
`book.pdf`:

```sh
cargo run --manifest-path tools/metrics-gen/Cargo.toml -- \
  --out "$out/metrics-pdf.smt" \
  --face "serif:normal:$pdf_font" \
  --face "math:normal:$pdf_math"
```

and passes `--metrics "$out/metrics-pdf.smt"` to the PDF render only. HTML, SVG
and JSON keep the embedded Noto table, because they are measured for a browser
or for cross-run comparison, not for the embedded PDF face. `SOMA_PDF_METRICS`
overrides the generated table; `SOMA_PDF_FONT_BOLD` adds a bold face.

The table is an artifact (written under `examples/book/out/`, ignored by git),
not a committed fixture: it belongs to the font the user picked. The binary
never opens a font.

## Consequences

- The PDF's breakpoints are measured with exactly the advances the PDF draws;
  mixed Latin/CJK lines no longer reserve Noto widths for MiSans glyphs.
- The page result changes when the table changes, through `metricsHash`; the
  CLI test `an_explicit_metrics_table_matches_the_embedded_one` proves that
  passing the committed table explicitly is byte-identical to `embedded()`.
- Generating a table is offline and cheap (one pass over cmap/hmtx); the
  embedded table remains the default for reproducibility.
- New CLI surface, no schema change: `soma.metrics/1` already carries a version
  and hash.

## Alternatives rejected

- **Parse the font in the CLI at layout time.** Duplicates `metrics-gen`
  parsing, pulls font tables into the layout path, and makes layout depend on a
  font file at runtime.
- **Make the core open fonts.** Violates the metrics seam (ADR-0010) and the
  `no_std` boundary.
- **Commit a MiSans table.** Font choice and licensing are the user's; the
  table is derived data.
- **Use Noto CJK in the PDF.** Those faces are CFF, which the PDF backend
  rejects (D82). Generating a table from whatever TrueType face is embedded is
  the portable answer.
