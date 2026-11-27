# Soma Studio (web preview)

Status: implemented, 2026-11-25; public deployment made static, 2026-11-27.
Design record: ADR-0093 (all-Rust wasm), ADR-0094 (HTML geometry as
percentages), ADR-0118 (browser-local mode) and ADR-0121 (static public
bundle).

The Studio is a desktop-class workbench for one project:

- **the whole engine runs in the browser** (`soma-studio-ui`, Leptos CSR):
  parsing, layout, pagination, proof, PDF and TikZ;
- **the public site is static**: Apache serves `dist/` and `/fonts/v2/`; there
  is no `/api` and no Axum process. `is_local()` defaults to local on every
  non-loopback host;
- **Axum is a development-only host** (`soma-studio`): file tree, image/font
  reads, native verification, PDF and a watcher on loopback;
- both paths call the same `soma-studio-core` build, and the canonical
  `PagedLayout::hash()` must be equal. D95's page-geometry half is closed; the
  remaining difference is glyph ink (D101).

## Build and run

```sh
# 1. the wasm UI (once, or after UI changes)
cd crates/soma-studio-ui
trunk build --release

# 2. the server (serves dist/ and the API)
cd ../..
cargo run -p soma-studio -- \
  --input examples/book/full.tex \
  --template examples/templates/plex.yaml \
  --metrics examples/templates/plex.smt \
  --font ~/.fonts/IBMPlexSansSC_Regular.ttf \
  --font-bold ~/.fonts/IBMPlexSansSC_Bold.ttf \
  --font-sans ~/.fonts/IBMPlexSansSC_Regular.ttf \
  --font-sans-bold ~/.fonts/IBMPlexSansSC_Bold.ttf \
  --font-mono /usr/share/fonts/noto/NotoSansMono-Regular.ttf \
  --font-mono-bold /usr/share/fonts/noto/NotoSansMono-Bold.ttf \
  --font-math /usr/share/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf \
  --font-fallback ~/.fonts/IBMPlexSansSC_Regular.ttf \
  --port 7070
```

Options:

| Flag | Meaning |
|---|---|
| `--input`, `-i` | The project input. Required. Its parent directory is the project root and the file tree. |
| `--template`, `-t` | Template YAML. Omitted: the built-in Plex reference. |
| `--engine` | `soma.engine/1` YAML. Omitted: the built-in `proposal-v1` preset (Chinese proposal defaults: tighter tolerance plus an eight-character short-tail threshold). The command-line tool still defaults to `latex-v1` (ADR-0069, ADR-0115). |
| `--metrics` | Metrics table. Omitted: the embedded `soma.metrics/7` table. In static mode the browser uses the embedded `plex.smt`; in development it can fetch the configured bytes from `/api/metrics`. |
| `--hyphenation` | TeX pattern file (e.g. `hyphen-en.pat`) for Liang hyphenation of Latin words. Omitted: the server uses `<project root>/hyphen-en.pat` when present, otherwise no word is broken. In static mode the browser uses the embedded `hyphen-en.pat`; in development it can fetch `/api/hyphenation`. |
| `--font`, `--font-bold`, `--font-sans`, `--font-sans-bold`, `--font-mono`, `--font-mono-bold`, `--font-math`, `--font-fallback` | Optional font files for PDF and for `@font-face` in the proof. `path#index` selects a TTC face. Math is bundled when omitted. When a body face is omitted, the server probes fontconfig (`fc-match`) for Noto Serif/Sans CJK SC and Noto Sans Mono; a face must cover `中` (or `A` for mono) to be accepted. Auto faces are referenced by family name in the proof and subset into the PDF, so a missing `--font` does not produce a blank export. |
| `--no-auto-font` | Disable fontconfig discovery; only explicit font flags are used. |
| `--dist` | UI build directory (default `crates/soma-studio-ui/dist`). |
| `--host` | Bind address, default `127.0.0.1` (loopback only). Use the machine's LAN address, e.g. `--host 192.0.2.10`, to serve other devices; the `/api` Origin allowlist follows it. `0.0.0.0` binds every interface and accepts the authority the browser used — see **Security**. |
| `--port` | Default 7070. |
| `--open` | Open the browser after binding. |
| `--no-watch` | Disable the file watcher and SSE reloads. |

The server prints a warning when `dist/index.html` is missing; run
`trunk build` first.

To serve another machine on the same network, bind a concrete LAN address and
use the same URL in the browser:

```sh
soma-studio --input … --host 192.0.2.10 --port 7070 \
  --dist /opt/soma-studio/dist
# then open http://192.0.2.10:7070/?server=1
```

Since ADR-0121, every non-loopback host defaults to the browser-local engine.
A LAN development session must opt into the server with `?server=1`; a public
host must not do this.

`--host` defaults to `127.0.0.1`, so omitting it keeps the local-only
behaviour. Loopback remains server mode by default; other hosts are local
unless the URL says `?server=1`. Use `--dist` when the binary runs outside the source checkout, and
keep the font and metrics paths absolute (or shipped next to the binary).

Mermaid `flowchart`/`graph` and Graphviz DOT fences are compiled by
`soma-diagram` into the same vector box as TikZ; the proof, the browser build
and the native verify/PDF path all draw them. Unsupported diagram syntax shows
its source, never a partial picture. `soma diagram --input chart.mmd` exports a
standalone diagram as TikZ; see [`docs/diagrams.md`](diagrams.md). The Studio
`⤴ TikZ` action exports the first diagram in the active tab at its natural
(unscaled) geometry, using the same template, metrics and `element: diagram`
style as the proof (ADR-0103).

## Views

Every tab is built with its own frontend. Opening `notes.md`, `page.html` or
`chapter.tex` inside a project previews that file through the shared `DocIR`
and the same layout engine; the project input only supplies defaults when no
tab is open.

| View | What it is |
|---|---|
| **分栏** (Split, default) | Source on the left, proof on the right, with a draggable splitter. Either pane can be closed with its `×`; the sidebar 「视图 → 分栏」 restores both. The ratio and open state persist in `localStorage`. |
| **源文** (Source) | A soft-wrapping `<textarea>`; a long line wraps at the pane width instead of scrolling horizontally. A diagnostics panel above it lists line/column, message and a suggested fix; exact spans get a red wavy underline (ADR-0122). Typing schedules a build 220 ms after the last keystroke; IME composition defers the build until `compositionend`. `⌘S` saves, `⌘↵` rebuilds now. |
| **校样** (Proof) | Each page is a Shadow DOM root containing one SVG page rendered from the engine's `PagedLayout` (ADR-0109). Text is `<text>` with `data-node`/`data-atom`, so it stays selectable and inspectable; the SVG is scaled by the UI's zoom. The page CSS is scoped inside the shadow root; UI tokens never leak into the document. |
| **检视** (Inspect) | The exported line facts for the current page: ratio, adjustment, trim, boundary deviation, cost, flags. Clicking a row selects the same line in the overlay. Per-term cost is not exported (D102); the detail bar shows `—`. |
| **校验** (Verify) | Static mode: the wasm build summary (canonical hash, parameter hash, pages, lines per page); native equality is a CI property. Development mode: wasm vs native, where a mismatch is a red state. |

Pane close buttons sit in each pane head; closing the last pane shows an empty-pane
state with “并排显示 / 只要源文 / 只要校样”. The source pane head intentionally
omits the file name and format badge: the tab already carries the file. The
page/zoom/overlay controls live in the proof pane head, and the verification
chip (static: `wasm 构建`; development: `wasm = native`) plus the “分栏”,
“TikZ” and “PDF” actions live in the title bar; there is no global breadcrumb
row.

The title bar also carries `⤓ PDF`, `⤴ TikZ` and `◫ 分栏`; PDF export works with
the proof closed, and the toast reports the page count and file size. A
downloaded PDF is never zero-length by construction (the UI rejects an empty
response). TikZ export needs a diagram in the active tab; a document without
one gets a readable error instead of an empty file.

### Source diagnostics (ADR-0122)

The source pane has a diagnostics strip above the editor. Each row shows
`Lline:column` (or `构建`), the message and `建议：…`. Clicking a source row
selects the exact span and focuses the editor; a build row without a source
span shows the suggestion as a toast.

The editor itself is still a textarea. A transparent text mirror with the same
font, padding, line-height and wrapping rules renders the source underneath;
diagnostic ranges get `text-decoration: underline wavy` plus a light red
background while the textarea keeps the visible text, caret, IME and selection.
Scroll positions are synchronised. The mirror is mounted only when at least one
diagnostic has a span.

The first lint set is deliberately conservative: unclosed fenced code blocks,
paired math parse errors (unbalanced group/delimiter/environment, unsupported
command, missing argument), undefined or duplicate footnote definitions,
unclosed `[[ruby:...]]` / `[[index:...]]` extensions, and unmatched LaTeX
`\begin{...}` / `\end{...}`. A build failure is always appended; when it names
`node N`, the pane re-parses the source and maps that node to a byte span.
Template, engine and metrics errors appear as `构建` rows without a source
underline, because they point at another file.

Line numbers are intentionally not shown: with soft wrap, one logical line can
occupy several visual rows, and a one-number-per-logical-line gutter would drift.
The pane head keeps the `line:column` cursor badge.

URL parameters (handy for screenshots and links):
`mode=split|source|proof|inspect|verify`, `page=N` (1-based), `theme=dark|light`,
`zoom=0.5…2`, `events=off` (disable SSE for headless capture),
`selftest=1` (run the browser gate), `local=1` (force browser-local on
loopback), `server=1` (force the development server on any host). `only=s9font`
restricts the self-test to the missing-font assertion.

### Static mode

On a public host the page opens `welcome.md` from the wasm and uses the
embedded template, engine parameters, metrics and hyphenation. The sidebar
「打开本地文件…」 action, the tab `+` button, the command palette and a window
drop open `.md`, `.html`, `.tex` or `.docx` without a server: text is decoded as
UTF-8, DOCX is parsed by `soma-docx` in wasm, and the bytes stay in the page.
`⌘S` downloads the current text (or the original DOCX archive). PDF and TikZ
are generated by the same wasm module. The only HTTP requests are same-origin
static assets under `/fonts/`.

## Font probe (D101)

The engine lays out from the `.smt` table; the browser draws with a font file.
The title bar shows a diagnostic pill:

- `字体一致` — p95 deviation < 5‰;
- `接近` — 5–20‰;
- `不一致` — > 20‰, or a template family is not installed.

The probe samples 16 characters (CJK, Latin, digits, punctuation) through
`soma-studio-core`'s `Build::probe`, measures the same characters with
`CanvasRenderingContext2D.measureText` at 100 px, and compares advances in
1/4096 em. It never feeds the breaker or the renderer; it is a signal, not a
layout input.

## Development server reloads (loopback only)

Static mode has no watcher. On the development host, with the watcher enabled
(default), a change to the input, template, engine or metrics file triggers a
native rebuild:

- success → SSE `{"type":"built","version":n}`. The UI reloads a clean tab,
  updates fonts/template/engine and rebuilds in wasm. Unsaved local edits win:
  the tab is left alone and a toast explains why.
- failure → SSE `{"type":"error","message":…}`. The server keeps the previous
  bundle; the UI keeps the previous proof and shows a red diagnostic banner
  plus a toast.

`POST /api/rebuild` forces the same path (and returns the new project payload).

## Security

- **Public static mode.** Apache serves `dist/` directly; there is no
  `ProxyPass`, no Axum process and no `/api`. `/api/health` is a 404. Layout and
  PDF run on the visitor's machine (ADR-0121).
- **Development mode only.** `/api` rejects cross-origin `Origin` and
  `Sec-Fetch-Site: cross-site` requests. Loopback origins are always accepted;
  `--host 192.0.2.10` additionally accepts `http://192.0.2.10:<port>`, and a
  wildcard bind accepts only the authority in the request's `Host` header.
  `Origin` validation is not authentication.
- **`--host` exposes read/write API routes.** There is no authentication: anyone
  who can reach the port can read and write files under the project root and
  download PDFs. Bind a concrete LAN address on a trusted network; do not bind
  `0.0.0.0` on an untrusted one. The default remains loopback-only. Do not put
  this server behind a public vhost.
- `Content-Security-Policy: default-src 'self'; …; script-src 'self'
  'unsafe-inline' 'wasm-unsafe-eval'; …; frame-ancestors 'none'` is added to
  every response, plus `X-Content-Type-Options: nosniff` and
  `Referrer-Policy: no-referrer`.
- The renderer emits no `<script>` and no `<a href>`; the proof uses Shadow DOM.
- The proof SVG carries `data-family`/`data-math`; the shadow CSS maps those to the
  installed `@font-face` families, so the browser does not reflow the layout.
- The assistant pane renders its markdown with raw HTML escaped and
  `javascript:`/`data:`/`vbscript:` destinations replaced. It has no write path
  to the engine (no `build`/`source`/`template` mutation, no breaker call).
- File reads and assets go through `safe_join`: absolute paths and `..` are
  refused.

## API surface (development host only)

The public site does not expose these routes. They exist only when
`soma-studio` is running, normally on loopback for development.

| Route | Method | Purpose |
|---|---|---|
| `/api/health` | GET | liveness |
| `/api/project` | GET | project payload: input, template, engine, fonts, capabilities, native summary |
| `/api/tree` | GET | project files (`.md`, `.markdown`, `.tex`, `.html`, `.htm`, `.docx`) |
| `/api/file?path=` | GET / POST | read / write a text source |
| `/api/source` | GET | binary source (DOCX) |
| `/api/asset?path=` | GET | image or other project asset |
| `/api/metrics` | GET | custom `.smt` table, when configured |
| `/api/hyphenation` | GET | TeX patterns, when configured or found as `hyphen-en.pat` |
| `/api/font/{role}` | GET | font bytes for `@font-face` |
| `/api/verify` | POST | native build summary for a `BuildRequest` |
| `/api/pdf` | POST | PDF for a `BuildRequest` |
| `/api/tikz` | POST | TikZ for the first diagram in a `BuildRequest` (natural size) |
| `/api/rebuild` | POST | reload the bundle from disk |
| `/api/events` | GET | SSE: `built` / `error` |

## Gates

```sh
bash scripts/studio.sh          # S1–S11, starts its own server on port 7091
SOMA_STUDIO_SKIP_TRUNK=1 bash scripts/studio.sh   # reuse dist/
SOMA_STUDIO_PORT=7099 bash scripts/studio.sh      # another port
bash scripts/static.sh          # static bundle: S1/S3–S10/S12–S14, zero /api
SOMA_STATIC_SKIP_TRUNK=1 bash scripts/static.sh   # reuse dist/
```

The browser assertions live in `crates/soma-studio-ui/src/selftest.rs` and are
read from `<pre id="selftest">` with Chromium `--dump-dom`. The script also
checks the CSP header, the PDF page count against `/api/verify`, SSE `built`
and `error`, and a second server without fonts (the probe must say `不一致`
and the proof must still render). The fixtures are created in a temporary
directory; `examples/` is not modified.

| Gate | What it locks |
|---|---|
| S1 | 38/30/24 chrome (no breadcrumb row); side head, tab bar and AI header share a 30 px baseline; default split pane; title-bar verification chip (static `wasm 构建`, development `wasm = native`); the status bar spans the window; one navigation tree. |
| S2 | Three files open as three tabs; reopening does not duplicate; closing the active tab selects the right neighbour; closing all shows the empty workspace; `+` opens the palette (development) or the file picker (static). |
| S3 | Sidebar 180–380, AI 320–900, persistence, double-click reset, collapse; closed AI dock has no content width; closing/reopening the source and proof panes. |
| S4 | Theme persists, both toggles work, and `studio.css` contains no hex values (tokens only). |
| S5 | `page slots == SVG fragments == page_count`; consecutive `data-page`; one Shadow `.page` SVG per slot; PDF page count equals native; `/api/tikz` returns a `tikzpicture` with the fixture labels. |
| S6 | Typing rebuilds after the debounce and changes the hash; IME composition does not rebuild until it ends. |
| S7 | Inspect rows equal the overlay lines on page 1; clicking a row selects the same box; unavailable fields display `—`. |
| S8 | No external resources; no `<script>`/`<a href>` inside page shadows; remote AI disabled; CSP and nosniff present. |
| S9 | Probe p95 with fonts and `不一致` without them; the proof's first block resolves to a dark ink colour (`color-scheme: light`); SSE `built`/`error`; a failed reload keeps the previous build; PDF page count. |
| S10 | The assistant has no engine write path; its reply renders as text; remote is off. |
| S11 | From a Markdown project, `extra.html` and `extra.tex` each rebuild with their own frontend: the wasm hash equals native verify, the proof shows the frontend marker, and raw `<h1>` / `\section` does not survive. |
| S12 | In static mode, the PDF is written by wasm: non-empty `%PDF-` bytes and a reported page count. |
| S13 | A browser `File` opens as an HTML tab, parses with the HTML frontend, builds, and closes without a server. |
| S14 | A source with an undefined footnote and an unsupported math command shows a diagnostics panel, red wavy spans on the exact bytes, a suggestion, and click-to-select in the textarea. |

## Known limits

- **No Worker yet (D104).** The build runs on the main thread; the 220 ms
  debounce keeps typing responsive for the project sizes tested here. Moving
  the build to a worker needs a serializable `AppBuild` and a second wasm
  binary, and is deliberately deferred.
- **No single-file export (D105).** The public Studio is a static bundle, but
  it still ships `index.html`, hashed JS/CSS/wasm and `/fonts/v2/` together.
  The fonts are not embedded in the wasm; a single-file offline build is
  deferred. The K-P slice (`web/`) remains the embeddable, server-free
  artifact.
- **Inspect shows total cost only** (D102), and line text is best-effort
  because atoms carry no source offsets (D103).
- **Source diagnostics are UI-side and conservative (D120).** Only the lint
  set above gets an underline; a build error that points at the template,
  engine or metrics table is shown as a `构建` row without a source span. The
  transparent mirror duplicates the source text while diagnostics exist, and
  its wrapping depends on matching the textarea's font and scrollbar width.
  A future structured-error pass in the engine can replace the heuristics
  without changing the panel contract.
- **The proof's ink is pinned to `#111`.** Page CSS sets `color:#111;color-scheme:light`
  so a dark browser preference can never turn text white on white paper; the
  proof therefore does not follow the UI theme's ink token. Templates may still
  colour individual runs.
- **PDF compatibility:** Firefox/pdf.js and Poppler render the CJK TrueType
  subset correctly; Ghostscript currently maps some CJK glyphs to the wrong
  shapes (D106). The PDF is not blank in the browser or in `pdftoppm`.
- **Diagrams are flowchart-shaped only (D107).** Mermaid `flowchart`/`graph`
  and a DOT subset are vector; sequence/class/state/ER diagrams and subgraph
  rank constraints are not implemented. A wide or too-tall diagram falls back
  to its source rather than overflowing the page.
- **TikZ export is first-diagram, natural-size only (D108).** Multi-diagram
  selection, page-scaled geometry and SVG export are not implemented.
- **AI is local rules only.** It reads the exported line facts and proposes
  template YAML; it does not call a network model.
