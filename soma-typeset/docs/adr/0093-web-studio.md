# ADR-0093: Web Studio is all-Rust wasm plus an Axum I/O backend

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0006 (determinism), ADR-0007 (frozen inline kernel),
  ADR-0034 (PDF backend), ADR-0071 (web K-P slice), ADR-0072 (emergency
  window), `docs/architecture-v2.md`, `docs/proposal-web-preview.md`
- Touches: new crates `soma-studio-core`, `soma-studio`, `soma-studio-ui`;
  `crates/soma-render-html` (page fragments); `docs/cli.md`, `docs/debt.md`,
  `docs/testing.md`, `README.md`; **not** the engine's canonical schemas,
  parameter sets, or layout behavior.

## Context

The first web plan used a vanilla-JS shell that consumed prebuilt artifacts,
with the 32 KB K-P slice as a paragraph loupe. That kept the browser small, but
it made the shell a second place where document state lives and left D95
("browser preview differs from PDF") as a permanent caveat.

The engine crates are `no_std + alloc` and were verified to compile for
`wasm32-unknown-unknown`:

```
soma-core soma-rules soma-metrics soma-template soma-layout
soma-md soma-html soma-latex soma-docx soma-math soma-tikz
soma-highlight soma-engine soma-render-html
```

They are deterministic, integer-only, and the embedded `soma.metrics/7` table
is already part of `soma-metrics`. There is no architectural reason to keep
layout on the server. Running the same code in the browser makes D95 a
testable equality instead of a product disclaimer.

## Decision

### One workspace, three web crates

| Crate | Target | Role |
|---|---|---|
| `soma-studio-core` | native + wasm | shared `Build` pipeline, page fragments, image sizing, font-name parsing, API DTOs |
| `soma-studio-ui` | wasm (Leptos CSR) | the workbench: shell, editor, proof, inspect, verify, AI pane |
| `soma-studio` | native (Axum) | project files, assets/fonts, PDF export, native verification, SSE, serving `dist/` |

The core engine crates stay dependency-free. Only the web layer uses
third-party crates (Leptos, Axum, serde, gloo, tower-http). `soma-studio-core`
is the only shared crate; it never touches the filesystem or the network.

### The whole engine runs in the browser

`soma-studio-ui` links `soma-studio-core`, which links the engine. On every
edit (debounced) the browser:

1. parses the source with the same frontend crate;
2. resolves the template and engine parameters;
3. lays out with `StyledMetrics` over the embedded `.smt` table;
4. renders HTML page fragments with `soma-render-html`;
5. computes `PagedLayout::hash()`.

The Axum server does the same build natively for `/api/verify` and
`/api/pdf`. The two hashes must be equal for the same `(source, template,
engine)`. This is the determinism gate for the web, not a promise in prose.

### Modes, not fidelity tiers

- **源文**: editor + the current block's layout from the last build.
- **校样**: the pages from the wasm `PagedLayout`, mounted in Shadow DOM.
- **检视**: the same pages plus the line table and parameter drawer.
- **校验**: wasm hash/page/line counts next to the native `/api/verify`
  result; a mismatch is a hard red state.

There is no "preview vs faithful" split: both sides run the same engine and
metrics. The remaining difference is glyph ink (the browser's font files vs
the PDF's embedded fonts), recorded as D101.

### Axum is the I/O boundary

`soma-studio --input <file> [--template …] [--engine …] [--font…]`:

- serves the Trunk `dist/` tree and falls back to `index.html`;
- `GET /api/project` returns paths, source/template/engine text, font URLs,
  capabilities, and the native summary;
- `GET /api/tree`, `GET|POST /api/file`, `GET /api/asset` are restricted to
  the project root (plus the explicit template/engine paths);
- `GET /api/font/:role` serves the same fonts the PDF backend uses;
- `POST /api/verify` builds natively and returns the canonical hash;
- `POST /api/pdf` renders the PDF with the native font stack;
- `POST /api/rebuild` re-reads files; `GET /api/events` (SSE) reports changes
  from a 500 ms mtime watcher;
- bind address is `127.0.0.1`; `/api` rejects a foreign `Origin`.

The UI is built with Trunk (`trunk build`); Axum serves the result. No npm,
no bundler, no JS glue. `trunk serve` may be used for UI iteration while Axum
keeps the API on `127.0.0.1:7070`.

### Shadow DOM for the paper

The engine's HTML is mounted in a Shadow DOM per page. The shell CSS cannot
reach the paper, and the engine's `em`-based coordinates scale with one
`font-size` on the host. The inspect overlay is a sibling layer in the same
shadow root; it uses the `PagedLayout` line boxes, not DOM measurements.

### D95 resolution

- **Page geometry and breakpoints: closed.** The same integer engine and the
  same `.smt` table run on both sides; `/api/verify` is the gate.
- **Glyph ink: D101.** The browser may lack a font; the proof still has the
  right boxes. `/api/project` exposes font files and family names so the app
  can inject `@font-face`; a font check is a diagnostic, not a layout source.
- **D102** still records that `Line` has total `cost` only; Inspect shows `—`
  for the unexported terms.
- **D103** still records that atoms have no source offsets; source mapping is
  block-exact and line-best-effort.
- **D104** records that the build runs on the main thread; the proposal's
  Worker offload is not part of this decision.
- **D105** records that the Studio has no single-file static export; the Axum
  boundary is the only runtime and `web/soma-kp.*` remains the embeddable
  slice.

## Consequences

- The browser bundle is larger than the 32 KB slice (engine + metrics table +
  Leptos). This is accepted: the Studio is a desktop-class workbench, not an
  embeddable widget. The `web/soma-kp.*` slice remains for embeds.
- The UI and server share `soma-studio-core`; a change to the build pipeline
  cannot drift between them.
- `soma-render-html` gains a fragment API (`style` + page body) used by Shadow
  DOM; `render`/`render_with` output stays byte-identical.
- `scripts/studio.sh` adds S1–S10. The existing 12 gates, `scripts/web.sh`,
  and the corpus baseline are untouched.
- PDF export remains server-only: fonts, flate and the PDF writer do not enter
  the wasm bundle.

## Alternatives

- **Vanilla JS shell + server layout.** Rejected by the all-Rust wasm
  requirement; it also split document state across JS and Rust.
- **Leptos SSR + hydration.** A valid stack, but the paper and editor are
  client-only; CSR keeps one build target and no hydration boundary.
- **Compile only the K-P slice into the UI.** Leaves pagination/tables/math
  server-side and keeps D95; rejected after the wasm check above.
- **Serve the UI from Trunk in production.** Adds a second server; Axum
  already owns the project boundary.

## Alignment

- `crates/soma-studio-core/` — `build`, fragments, image sizing, DTOs.
- `crates/soma-studio-ui/` — Leptos app, `style/tokens.css`,
  `style/studio.css`, Trunk `index.html`.
- `crates/soma-studio/` — Axum server, PDF, verify, SSE, static dist.
- `crates/soma-render-html/src/lib.rs` — page fragment API.
- `scripts/studio.sh` — S1–S10.
- `docs/proposal-web-preview.md`, `docs/cli.md`, `docs/debt.md`,
  `docs/testing.md`, `README.md`.
