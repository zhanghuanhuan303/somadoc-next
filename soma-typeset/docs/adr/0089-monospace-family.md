# ADR-0089: Monospace is a third font family

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0037 (metrics tables), ADR-0038 (mixed fonts), ADR-0069
  (archived article template), ADR-0090 (code block panel)
- Touches: `soma-metrics` (`FAMILY_MONO`), `soma-template` (`family_role`),
  `soma-layout` (face selection),
  `soma-render-pdf` (`Role::Mono`, `PdfFonts`), `soma-cli` (`--font-mono`,
  `--font-mono-bold`), `tools/metrics-gen`, `scripts/plex.sh`,
  `crates/soma-template/tests/a2_units.rs`, `crates/soma-render-pdf/tests/pdf.rs`

## Context

The style layer had two text slots: serif and sans. `font-family: mono` was
folded into sans because the renderer had no third face. That was acceptable
for inline code inside a Chinese body face, but it cannot reproduce the Rust
book code block: a proportional face destroys column alignment, makes
operators and indentation hard to scan, and breaks the visual rhythm of code.

The metrics blob already stores a face family byte. The generator and parser
read it generically, so adding a family code does not need a blob layout
change; old blobs simply have no mono face.

## Decision

### Style vocabulary

`font-family` resolves to three roles:

- `serif` (default): Songti, Noto Serif CJK, Latin Modern Roman, …
- `sans`: Heiti, IBM Plex Sans SC, Noto Sans CJK, …
- `mono`: Iosevka, IBM Plex Mono, Noto Sans Mono, …

`mono`, `monospace`, `code` and `typewriter` select the mono role.
`family_is_sans` remains as a compatibility predicate for code that only needs
"not the body face"; it is now false for mono. The canonical family string is
unchanged, so templates keep their exact bytes.

### Metrics

`soma.metrics` gains `FAMILY_MONO = 4`. `FaceSpec` in `tools/metrics-gen`
accepts `--face mono:normal:<path>` and `--face mono:bold:<path>`. The blob
version stays 7: the face record is self-describing and an older reader that
does not know family 4 still skips it by its length fields.

Face selection in layout:

1. mono + weight, if the table has it;
2. otherwise sans + weight (a mono template still renders in a sans-only
   table);
3. otherwise the first face.

The same "mono, else sans, else body" order applies to the PDF's declared
font roles. If `--font-mono` is omitted, `Role::Mono` falls back to the sans
face, so an existing command line keeps working.

### PDF

`PdfFonts` gains `mono` and `mono_bold` optional fields; `Role` gains `Mono`
and `MonoBold` with resource names `F7`/`F8`. The fallback role remains `F6`.
The CLI gains `--font-mono` and `--font-mono-bold`, both optional.

CJK text inside a mono run is handled by the existing rescue-face path
(ADR-0038): the metrics table's fallback face measures it, and the PDF uses
`--font-fallback` for the glyphs. This is why the sample script registers the
body Plex face as the fallback.

## Consequences

- The Plex template sets `family: mono` on `codeBlock` and `code`; those runs
  now use the third face instead of being drawn with Plex Sans SC.
- A metrics table without a mono face keeps working; code is proportional but
  colored and boxed.
- `scripts/plex.sh` measures Noto Sans Mono as the mono face and passes it to
  the PDF backend. IBM Plex Mono is used instead when the user sets the
  environment overrides.
- A PDF may now embed two extra font subsets. Only faces that actually draw a
  glyph are embedded.
- The archive `latex-article.yaml` still says `family: mono`; it now gets a
  real monospace face, which is closer to TeX's `\ttfamily` than before.

## Alternatives

- **Reuse sans for mono.** Column alignment is the point of code; rejected.
- **Make mono a variation of sans inside one face.** A proportional face cannot
  be made monospaced by tracking; rejected.
- **Bump the metrics blob version.** The face record is already length-prefixed
  and self-describing; a version bump would invalidate every checked-in table
  and hash for no parsing gain; rejected.
- **Require `--font-mono` for PDF.** It would break existing invocations;
  fall back to sans instead.

## Alignment

- `crates/soma-template/src/style.rs` — `FamilyRole`, `family_role`,
  `family_is_sans`.
- `crates/soma-metrics/src/lib.rs` — `FAMILY_MONO`.
- `crates/soma-layout/src/metrics.rs` — face selection.
- `crates/soma-render-pdf/src/lib.rs` — `Role`, `PdfFonts`, `role_for`.
- `crates/soma-cli/src/main.rs` — font flags and `PdfFonts` construction.
- `tools/metrics-gen/src/main.rs` — `mono` family token.
- `scripts/plex.sh` — mono face measurement and PDF flags.
