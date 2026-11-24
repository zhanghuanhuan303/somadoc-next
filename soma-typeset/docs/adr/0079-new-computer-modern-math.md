# ADR-0079: The default math face is New Computer Modern Math

- Status: Accepted; supersedes the face choice of ADR-0078. The CFF math
  pipeline and the bundled-face mechanism remain.
- Date: 2026-11-24
- Related: ADR-0070 (math v2), ADR-0075 (design variants), ADR-0076
  (Typst-aligned geometry), ADR-0077 (probe and advance check), ADR-0078
  (CFF math ink and the bundled face)
- Touches: `crates/soma-metrics/data/noto-cjk.smt`,
  `examples/book/metrics-lxgw.smt`, `crates/soma-cli/assets`, `soma-cli`
  (probe order), `scripts/math-face.sh`, `PROVENANCE.md`, `docs/`

## Context

ADR-0078 replaced Noto Sans Math with Latin Modern Math. A side-by-side with
the user's reference `somadoc-16.pdf` — a Typst 0.13.1 document — showed that
Typst draws formulas with **New Computer Modern Math**, not Latin Modern.
Both are Computer Modern descendants, but NewCM is Typst's default face and
the reference's exact shapes: a slightly larger x-height, sturdier stems, and
more design variants (4283 math glyphs, 490 vertical variants, 41 `MATH`
constants in the installed version 4.0) than Latin Modern (2022 glyphs, 353
variants). The CFF pipeline from ADR-0078 already measures and embeds it, so
the change is a table regeneration plus the bundled asset.

## Decision

1. **The embedded table's math face is New Computer Modern Math.**
   `noto-cjk.smt` keeps its Noto Serif/Sans CJK body faces and measures
   `NewCMMath-Regular.otf` instead of Latin Modern. `metrics-lxgw.smt` likewise.
2. **The bundled face is `crates/soma-cli/assets/NewCMMath-Regular.otf`**
   (1,187,476 B, GUST Font License, SHA-256
   `60394d357348f68cd301764fe61cc502a5858e1c4ff21b948a1d14d82586a7a2`).
   The previous `latinmodern-math.otf` asset is removed. The license text stays
   in `crates/soma-cli/assets/GUST-FONT-LICENSE.txt`.
3. **Probe order becomes NewCM first**, then Latin Modern, Noto Sans Math,
   DejaVu Math TeX Gyre, TeX Gyre Pagella Math, Libertinus Math, STIX Two Math
   and Cambria Math. The advance check from ADR-0077 still decides; an explicit
   `--font-math` is still trusted.
4. **`scripts/math-face.sh` prefers NewCM**, then Latin Modern/DejaVu/Noto, so
   the sample scripts and the CLI default agree.

## Consequences

- The default PDF draws formulas with the reference face. The two-page
  `math-demo` sample embeds `SomaMath` as CID Type 0C; `--stats` reports
  `math: built-in New Computer Modern Math (auto)`.
- Table sizes: `noto-cjk.smt` 307,776 B (Latin Modern was 275,744 B);
  `metrics-lxgw.smt` 400,390 B (368,358 B). The CLI asset grows from 733,736
  to 1,187,476 B.
- Corpus page and line counts are unchanged; the scorecard is identical to the
  Latin Modern run (`math-page` jitter 398, `ALL` 427, `fitness_jumps` 6).
- The canonical hash of a given math document changes (different advances and
  variants); every sample is regenerated.
- A machine without NewCM still gets it from the bundle. A custom table
  measured from another math face falls through to the installed probe.

## Alternatives

- **Keep Latin Modern Math.** It is a valid CM design and was the previous
  default, but it is not the face the reference uses and has fewer design
  variants. Rejected as the default; it remains a probe candidate.
- **Use Noto Sans Math or DejaVu Math.** Sans or TeX-Gyre designs, neither is
  the Typst/LaTeX reference. Kept as fallbacks.
- **Do not bundle NewCM.** The default would depend on TeX Live again, which
  ADR-0078 removed. Rejected.

## Alignment

- `PROVENANCE.md` — NewCM source, SHA-256, GUST Font License and the bundled
  asset.
- `docs/math.md`, `docs/fonts.md`, `docs/cli.md`, `README.md`, `docs/debt.md`.
- Tests: the same suite from ADR-0077/0078; the embedded face is checked
  against the table before use.
