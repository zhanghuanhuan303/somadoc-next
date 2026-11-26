# ADR-0086: Renderers distribute line leading around the baseline

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0014 (typography refinements), ADR-0081 (reference template),
  ADR-0084 (table rule separation)
- Touches: `soma-render-pdf`, `soma-render-svg`, `soma-render-html`
  (inline math baseline), `tools/metrics-gen` (`--leading`),
  `scripts/plex.sh`, `crates/soma-render-svg/tests/gate_r.rs`,
  `crates/soma-render-*/tests/inline_math.rs`

## Context

`Line` reports `ascent`, `descent`, `leading` and `height`; `docs/kp-model.md`
already says that the distribution of `leading` is the renderer's decision
("half-leading 等"). The renderers instead did:

```text
baseline = line top + ascent
```

so the whole leading sat **below** the baseline. In a table cell this made the
text visibly high between two rules: at a 10.5pt body with Plex, the line was
20pt tall, the ink 10pt, and the remaining 10pt all fell below the text. In the
reference PDF, the air is split half above and half below the ink.

Plex Sans SC also declares a one-em `lineGap`. The core takes
`max(ascent + descent + leading, style.line_height)`, so the template's
`line-height: 1.44` was ineffective and body lines were 2em apart. LaTeX's
`\linespread{1.2}` ignores the font's lineGap; the reference body is 14.4pt.

## Decision

1. **Renderers place the baseline at `top + ascent + leading / 2`.** This
   applies to PDF text, SVG text, the SVG/HTML inline-math baseline, footnote
   numbers, and the PDF marker/decoration geometry derived from the baseline.
   The body line **pitch** does not change; only the glyph's position inside its
   line box does.
2. **HTML text remains CSS-positioned.** The browser already applies
   half-leading to its own line box; only the absolutely positioned inline math
   in HTML adds the same half-leading to stay on the text baseline.
3. **The font's lineGap can be overridden at metrics generation.**
   `tools/metrics-gen --leading <i32>` replaces every face's leading (1/4096
   em). The Plex sample passes `--leading 0` because the reference's baseline
   grid comes from `\linespread{1.2}`, not from the font's recommendation.
   This is a measurement choice, not a template knob; the engine still takes
   `max(content, style.line_height)`.

## Consequences

- Text is vertically centred inside its line box: a table cell between two
  rules now has equal air above and below the ink.
- With `--leading 0`, the Plex sample's body, table rows and footnotes use the
  template's 1.44 line height, matching the reference's 14.4pt baseline.
- The layout heights do not change; only renderer coordinates. Canonical page
  geometry and page breaks are untouched. PNG/SVG visual hashes change.
- The SVG/HTML inline-math tests and the gate R coordinate expectations add
  `leading / 2` to the baseline.
- The PDF footnote marker and footnote number are raised from the corrected
  baseline; the 1/5em raise remains.

## Alternatives

- **Change `Line.height` to ignore font leading globally.** It would alter the
  core contract and every existing page/hash, and the core already documents
  leading as renderer-distributed; rejected.
- **Add cell-specific padding to compensate.** It makes body rows tall and only
  fixes one of the three renderers; rejected.
- **Leave the font lineGap and accept 2em lines.** The reference and the
  template both say 1.44; rejected.

## Alignment

- `crates/soma-render-pdf/src/lib.rs` — text baseline and footnote number.
- `crates/soma-render-svg/src/lib.rs` — text and inline math baselines.
- `crates/soma-render-html/src/lib.rs` — inline math baseline.
- `tools/metrics-gen/src/main.rs` — `--leading`.
- `scripts/plex.sh` — `--leading 0` for the reference sample.
- `crates/soma-render-*/tests/inline_math.rs` — expected baselines.
