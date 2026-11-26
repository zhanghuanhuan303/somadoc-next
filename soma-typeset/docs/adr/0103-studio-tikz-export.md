# ADR-0103: Studio exports the compiled diagram as TikZ

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0102 (diagram compiler), ADR-0092 (TikZ vector subsystem),
  ADR-0093 (web Studio)
- Touches: `soma-studio-core` (`diagram_tikz`), `soma-studio` (`/api/tikz`),
  `soma-studio-ui` (title-bar action), `scripts/studio.sh`, `docs/studio.md`,
  `docs/diagrams.md`

## Context

ADR-0102 gave `Block::Diagram` a semantic source and a compiler that produces
the same integer vector box as a hand-written `tikzpicture`. The CLI already
exposed `soma diagram --input chart.mmd`, but a Studio user editing a document
had no way to take the compiled figure out of the proof. The common request is
"give me the TikZ for this flowchart" so the figure can be pasted into a LaTeX
paper, reviewed in source form, or compared with the page.

The export must be reproducible and must not introduce a second diagram
compiler. It also must not promise page-scaled geometry: the page may have
scaled a wide diagram down to fit the measure, while a standalone TikZ export is
most useful at the diagram's natural size.

## Decision

### Shared core

Add `soma_studio_core::diagram_tikz`. It:

- parses the active frontend through the normal `parse_document`;
- resolves the same template and metrics table as the proof;
- finds the **first** `Block::Diagram` in document order, including diagrams
  nested in lists, block quotes, table cells and generated blocks;
- uses that node's computed style (`styled.style_of(node.meta.id)`), so label
  size, line height and colour match the page even when `element: diagram`
  differs from the body style;
- runs `soma_diagram::layout` and returns `soma_diagram::to_tikz`.

The coordinates are the diagram's natural, unscaled geometry. The layout's
60%-scale / page-height fallback applies only to the page display; a diagram
that is too wide for the current template still exports as valid TikZ. The
export is never synthesized from rendered output and never emits a partial
picture: a parse or layout error returns an error.

### Server and UI

- `POST /api/tikz` accepts the same `BuildRequest` as `/api/pdf` and
  `/api/verify`. It returns `text/plain; charset=utf-8` with a
  `soma-diagram.tikz` attachment name. A document without a diagram, or a
  diagram that fails to compile, is `422 Unprocessable Entity` with a readable
  message.
- The title bar gains `⤴ TikZ` next to `⤓ PDF`, plus a command-palette item
  "导出 TikZ（第一张流程图）". The browser downloads the server response
  directly; the wasm build does not duplicate the export logic.
- Only the first diagram is exported. This is stated in the action title and
  the documentation; per-figure selection is deliberately deferred until the
  document model carries stable figure targets.

### Gates

`scripts/studio.sh` posts the existing Mermaid fixture to `/api/tikz` and
asserts the response contains `\begin{tikzpicture}` and the fixture's labels.
The shared export has a Rust test in `crates/soma-studio-core/tests/studio.rs`;
the native/wasm hash gates are unchanged because TikZ export does not alter a
build.

## Consequences

- A Studio user can inspect or reuse the natural vector geometry the engine
  computed, without a second Mermaid/DOT implementation and without a font or
  browser round trip.
- The export is deterministic and native-only. It is not part of the wasm
  layout path, so it cannot change canonical hashes.
- The response can be large for a diagram near the compiler limits; the UI
  reports the size and lets the browser save it as a file.
- The endpoint does not export other figures, diagrams after the first one, or
  the page's scale factor. Those need a document target model and a separate
  decision.
- `/api/tikz` inherits the same Origin allowlist and loopback default as the
  other Studio APIs.

## Alternatives

- **Export in the browser.** Rejected: it would duplicate diagram compilation in
  wasm and require the UI to carry the template/metrics seam that the server
  already owns.
- **Export the page-scaled box.** Rejected: a standalone figure is more useful
  at its natural size, and a scale factor belongs to one page's measure, not to
  the figure's source.
- **Add a per-figure "export" button in the proof.** Deferred: the proof
  currently exposes page geometry, not stable diagram identities. The first
  diagram plus a clear error is enough for the MVP.
- **Emit SVG.** Rejected: SVG already exists as a renderer, but the request is
  source-level TikZ that a LaTeX author can edit.
