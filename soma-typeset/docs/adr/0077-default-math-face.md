# ADR-0077: The default math face is probed and proved against the metrics table

- Status: Accepted; the default math face is superseded by ADR-0079
  (New Computer Modern Math, bundled with the CLI). The probe and advance check
  remain.
- Date: 2026-11-24
- Related: ADR-0012 (table metrics), ADR-0070 (math v2), ADR-0075
  (vertical variants addressed by ordinal), ADR-0076 (Typst-aligned geometry),
  `docs/math.md` §Metric source, `docs/cli.md`
- Touches: `soma-render-pdf` (`advance_4096`, `has_glyph`), `soma-metrics`
  (`advance_recorded`, `MathFace::glyph_at`, `Face::math_matches`), `soma-cli`
  (probe, `--stats` line, warning), `docs/`

## Context

`--format pdf` required `--font-math`. When the flag was omitted, layout still
took every formula measurement from the table's math face, but the renderer
drew the same items with the body face. The two disagreed on advances, on the
display variants (the table addresses them by ordinal, ADR-0075) and on the
ink the limits were placed from. A text-only run hid the defect because no
formula reached the page; `examples/book/out/math-typst.pdf` was produced with
the flag, so the default path was never exercised.

The metrics table stores numbers only. `PROVENANCE.md` states that the
repository redistributes no font binaries, and a table carries no face name or
file path. Picking the "first math font on the machine" by file name is unsafe:
in `docs/math.md` a table built from Noto Sans Math and drawn with DejaVu still
resolves every ordinal, but a display `∑` is 1.34 em in one and 1.51 em in the
other, and the limits sit where the other font's ink put them.

## Decision

1. **An explicit `--font-math` is authoritative.** It is loaded as given and is
   not verified: the documented contract is "the table and this face were
   measured together", and a caller may knowingly use a near-metric face.
2. **When the flag is omitted and the document contains at least one formula**,
   the CLI tries an ordered list of installed math faces (Noto Sans Math,
   DejaVu Math TeX Gyre, TeX Gyre Pagella Math, Latin Modern Math, Libertinus
   Math, STIX Two Math, Cambria Math) at their usual system paths. A candidate
   is accepted only when **at least 16 sampled advances agree exactly** with the
   table's recorded values in 1/4096 em. Sampling walks the table's
   codepoint-sorted math rows evenly, so agreement on Latin letters alone
   cannot pass a face whose symbols differ. Codepoints the table does not
   record explicitly are skipped rather than compared with the 4096 default.
3. **No candidate matches, or no formula is present:** no probe runs, no font
   is loaded, and rendering is unchanged. When the document *does* contain a
   formula, a warning names the fix and says formula symbols use the body face.
   A missing or unparseable candidate is skipped silently; a broken font
   install never fails a render.
4. **`--stats` reports the resolution**: `math: <spec> (auto)`,
   `math: <spec> (explicit)`, or `math: none` on stderr, next to the existing
   page/line summary.
5. **No font binary is redistributed.** The probe reads files that are already
   installed and never modifies them.

## Consequences

- The embedded table is measured from Noto Sans Math, so a machine that has
  that file gets a matching math face by default. A table generated from
  DejaVu Math (`scripts/full.sh`, `scripts/fonts.sh`) matches DejaVu; a custom
  face matches only itself. `--font-math` still overrides everything.
- The default and explicit paths produce **byte-identical PDFs** on this
  machine: the two-page `math-demo` sample is 909 313 bytes either way and
  shares `canonicalHash=76f035cde7aeebd1`.
- A machine with no matching math face keeps the old body-face fallback, now
  with a stderr warning that says which flag fixes it.
- The check is advance-based, not name-based: a metrically identical clone is
  accepted (it lays out correctly), while another cut of the same family is
  rejected.
- The probe reads at most 48 codepoints per candidate, and only when a formula
  exists, so a text-only document pays nothing.
- On macOS and Windows the listed paths are best-effort. A user whose math face
  lives elsewhere passes `--font-math`; the warning is the discoverable cue.

## Alternatives

- **Record a face name or fingerprint in the table (a v8 blob).** A schema bump,
  and a name still has to be resolved to a file. Rejected for now; the
  advance comparison needs no new bytes.
- **Embed Noto Sans Math in the CLI.** Guarantees the default everywhere, but
  redistributes a font binary, which `PROVENANCE.md` deliberately avoids, and
  grows the binary by about 1 MB. Rejected.
- **Scan every installed font.** No portable enumeration without a dependency,
  and parsing a whole font directory on every render is the wrong price.
- **Match on family or file name.** A name is not a metric; different builds,
  subset fonts and renamed cuts keep the name and change the advances.
- **Warn on an explicit mismatch.** Noisy for the documented override contract,
  and the CLI cannot know whether the caller measured a different table on
  purpose. The explicit path stays trusted.

## Alignment

- `docs/math.md` — Metric source and the ordinal paragraph.
- `docs/cli.md` — `--font-math`, `--stats`.
- `docs/fonts.md` — the probe list under automatic selection.
- Tests: `crates/soma-metrics/src/lib.rs` (`Face::math_matches`),
  `crates/soma-render-pdf/tests/pdf.rs` (`advance_4096`), and
  `crates/soma-cli/tests/cli.rs` (`an_installed_math_face_is_chosen_without_the_flag`).
