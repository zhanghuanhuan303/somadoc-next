# ADR-0108: Font-selection maths commands, matrix gutters, and `<details>`

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0024 (HTML subset), ADR-0104 (mixed Markdown), ADR-0075
  (math design variants), ADR-0107 (mixed sections/footnotes)
- Touches: `soma-math` (`lib.rs`, `tests/math.rs`), `soma-html` (`lib.rs`,
  tests), `soma-md` (`embedded.rs`, tests)

## Context

A mixed Markdown sample exposed three formatting failures:

1. `$\LaTeX$` and `$$\mathbf{A} = \begin{pmatrix}…\end{pmatrix}$$` fell back to
   the formula source as plain text. The math parser rejected the unknown
   commands `\LaTeX` and `\mathbf`, and one unsupported command discards the
   whole `MathBox`.
2. A three-column `pmatrix` used the relation gap between columns. Digits have
   almost no side bearing, so `1 2 3` read as `123`.
3. `<details><summary>…</summary>…</details>` leaked its tags into the page.
   `details` was neither a block nor a transparent tag, so it was treated as
   inline content; the summary and body were merged into one paragraph.

A separate observed difference — `style="border:…"` and
`style="color:red"` not taking effect — is not a bug: the HTML frontend is a
semantic whitelist and presentation comes from the template (ADR-0024).

## Decision

### Math font commands are transparent

`\mathbf`, `\mathrm`, `\mathit`, `\mathsf`, `\mathtt`, `\mathcal`,
`\mathbb`, `\mathfrak` and `\boldsymbol` parse their group and keep the body.
The math face has no bold/roman variant yet, so the body keeps its normal math
shape; the important part is that the formula still lays out. `\LaTeX` and
`\TeX` become ordinary text symbols (`LaTeX`, `TeX`), and `\varepsilon` is an
alias for `ε`.

### Matrix gutter

A non-`aligned` matrix uses `max(rel_gap, 0.5em)` between columns. TeX's
`\arraycolsep` is wider than a relation gap, and a half-em gutter is the
smallest value that keeps a three-column numeric matrix readable.

### `<details>` and `<summary>`

Both become block tags. `details` is a transparent block container; `summary`
becomes a paragraph whose content is wrapped in `Inline::Strong`. A static PDF
cannot fold, so the body is laid out expanded, in document order. The HTML
renderer also shows it expanded; the tags themselves never reach the page.

## Consequences

- A formula with a font-selection wrapper no longer collapses to source.
- `\mathbf` is visually not bold until the math face carries a bold variant;
  this is recorded as a visual limitation, not a parse failure.
- Matrix rows gain a visible gutter; corpus line/page counts are unchanged.
- `<details>` content participates in pagination normally; a short trailing
  page is possible, but the tags and the inline merge are gone.
- Inline `style` remains intentionally ignored; authors use template selectors
  or semantic tags (`strong`, `em`, `code`) for presentation.
