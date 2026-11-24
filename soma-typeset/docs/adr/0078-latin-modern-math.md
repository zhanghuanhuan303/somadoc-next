# ADR-0078: The default math face is Latin Modern Math; CFF faces are measured and embedded

- Status: Accepted
- Date: 2026-11-24
- Related: ADR-0012 (table metrics), ADR-0039 (CFF embedding), ADR-0070 (math
  v2), ADR-0073 (CFF punctuation ink), ADR-0075 (design variants), ADR-0076
  (Typst-aligned geometry), ADR-0077 (default face resolution)
- Touches: `tools/metrics-gen` (CFF math ink, Type 2 curve fix, stem counting),
  `soma-render-pdf` (CFF subsetter), `crates/soma-metrics/data/noto-cjk.smt`,
  `examples/book/metrics-lxgw.smt`, `crates/soma-cli/assets` (bundled face),
  `soma-cli` (probe order), `PROVENANCE.md`, `docs/`

## Context

ADR-0077 made `--font-math` optional: the CLI probes installed faces and accepts
only one whose advances agree with the metrics table. The embedded table was
measured from **Noto Sans Math**, a sans face. With a Chinese serif body the
formulas read as a different voice; the LaTeX default look is a serif math face,
and the natural choice is **Latin Modern Math**, the Computer Modern successor.

Latin Modern Math has CFF outlines. Two pieces of the engine assumed a `glyf`
math face:

1. `tools/metrics-gen` recorded a math glyph's vertical ink only when the face
   had a `glyf` table. Without that ink a radical cannot sit on its radicand, an
   operator cannot be centred on the axis, and an accent's signed ink cannot be
   used. The CFF Type 2 interpreter already existed for punctuation ink
   (ADR-0073), but it was not applied to the math face.
2. The PDF backend's CFF subsetter counted stems as `stack.len() & !1` — twice
   the real count — when skipping a `hintmask`. A charstring stream with masks
   then landed on the wrong byte and failed with “moveto has too few operands”.
   Latin Modern Math uses hint masks heavily; Source Han Serif subsetting had not
   exercised the path enough to expose it.

While comparing Latin Modern Math's radical bbox against FreeType, a third bug
surfaced: `vhcurveto` / `hvcurveto` put the first argument on the wrong axis
(`dx1` instead of `dy1`, and vice versa). It made the base radical's ink box
sit below the baseline. FreeType reports `radical` at
`(73, -960, 853, 40)`; the corrected interpreter matches it exactly.

## Decision

1. **The embedded table's math face is Latin Modern Math.** `noto-cjk.smt` keeps
   its Noto Serif/Sans CJK body faces and replaces Noto Sans Math with Latin
   Modern Math. `metrics-lxgw.smt` likewise. The table stores numbers only.
   The same face is bundled as `crates/soma-cli/assets/latinmodern-math.otf`
   (GUST Font License, license text next to it), so the default does not depend
   on a TeX installation. The bundled face is used only after the advance check
   passes; a custom table measured from another math face falls through to the
   installed probe.
2. **A CFF math face records vertical ink.** The generator runs the Type 2
   charstrings for every math glyph; a glyph the interpreter cannot measure is
   skipped rather than failing the table. `vhcurveto` / `hvcurveto` follow the
   Type 2 operand layout, and `hintmask` counts stems as `len / 2` in both the
   generator and the PDF subsetter.
3. **The probe tries the bundled face first**, then Latin Modern Math from
   installed paths, Noto Sans Math, DejaVu Math TeX Gyre, TeX Gyre Pagella Math,
   Libertinus Math, STIX Two Math and Cambria Math. The advance check from
   ADR-0077 still decides; an explicit `--font-math` is still trusted as given.
4. **Body punctuation ink is regenerated too.** The CFF body faces go through the
   same corrected interpreter. This changes trim values (for example the
   exclamation mark's side bearings), but not the corpus page or line counts; it
   is a correctness fix, not a style choice.

## Consequences

- The default PDF now draws formulas with Latin Modern Math (serif, LaTeX
  design). The two-page `math-demo` sample embeds `SomaMath` as CID Type 0C and
  `pdffonts` reports it; `∑`, `√`, fractions, delimiters and matrices match the
  LaTeX shapes.
- The table shrinks from 301,460 to 275,744 bytes (Noto's 2901 math glyphs and
  963 variants give way to 2022 glyphs and 353 variants). The LXGW baseline
  shrinks from 394,074 to 368,358 bytes.
- Corpus page/line counts are unchanged. The scorecard's jitter changes with the
  new trim ink and math advances: `math-page` 473 → 398, `ALL` 377 → 427; the
  other documents move by at most 90. `fitness_jumps` on `ALL` drops 8 → 6
  because the old comma/quote trim was over-trimming. `docs/corpus-calibration.md`
  records the new row.
- A machine with no Latin Modern Math still gets the LaTeX design for the
  embedded table: the face is bundled with the CLI. TeX Live installs the same
  file under `texmf-dist/fonts/opentype/public/lm-math`; a custom table measured
  from another face ignores the bundle (it fails the advance check) and probes
  the installed paths, then warns if none matches.
- The CLI binary grows by about 734 KB before compression. The GUST license
  ships with the font in `crates/soma-cli/assets`, and `PROVENANCE.md` records
  the source hash.
- CFF math faces other than Latin Modern (Pagella, STIX Two, Libertinus OTF) can
  now be used as the measured face; the glyf-only restriction is gone.
- An explicit `--font-math` that disagrees with the table is still trusted, so a
  caller can compare designs knowingly.

## Alternatives

- **Keep Noto Sans Math.** No engine changes, but the formulas stay sans and the
  request for the LaTeX look is unmet. Rejected as the default; it remains a
  probe candidate for tables measured from it.
- **Use DejaVu Math TeX Gyre.** A glyf face, so no CFF work, but its design is
  DejaVu's, not Computer Modern. It stays in the probe list and in
  `scripts/*.sh` fallbacks.
- **Do not embed Latin Modern Math in the CLI.** The default would silently
  depend on TeX Live (or on a user's `--font-math`), which is the problem this
  ADR exists to remove. Rejected.
- **Estimate the CFF math ink instead of measuring it.** The radical link,
  operator centring and accents regress to the estimates ADR-0075/0076 removed.
  Rejected.

## Alignment

- `PROVENANCE.md` — math face source, SHA-256, GUST Font License.
- `docs/math.md` §Metric source, `docs/fonts.md`, `docs/cli.md`, `README.md`.
- `docs/corpus-calibration.md` — the new ALL row and per-document jitter.
- Tests: `soma-metrics` (accent attachment is signed), `soma-cli`
  (auto-probe embeds a CFF math program), `soma-render-pdf` (CFF subsetter and
  `advance_4096`).
