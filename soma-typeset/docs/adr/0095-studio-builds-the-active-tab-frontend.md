# ADR-0095: Studio builds the active tab with its own frontend

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0093 (Web Studio), ADR-0044 (DocIR), ADR-0064 (frontends)
- Touches: `soma-studio-core`, `soma-studio-ui`, `soma-studio`, the S gate;
  no canonical layout, PDF, SVG or corpus change

## Context

A Studio project is opened with one input file, and `ProjectPayload.format`
records that file's frontend. Tabs can open any project file, including a
different frontend, but both the browser build (`engine::build`) and the
server requests (`/api/verify`, `/api/pdf`) read `project.format`. Opening
`notes.md` inside a LaTeX project therefore parsed Markdown as LaTeX: the
preview showed the raw markers, and `wasm = native` could agree on the wrong
document. The engine itself was fine — `soma_studio_core::parse_document`
already dispatches Markdown, HTML, LaTeX and DOCX into the same `DocIR`.

## Decision

- **The active tab owns the frontend.** `DocTab.format` is passed to the wasm
  build. `rebuild_now`, `verify_now` and `export_pdf` resolve the format from
  the active tab and fall back to the project input only when no tab exists.
- **`BuildRequest` carries an optional `format`.** The UI always sends the
  active tab's format. The server calls `BuildRequest::format_or(project)`, so
  an older client that omits the field keeps the previous behaviour and the
  initial project load is unchanged.
- **One format flows through both paths.** The browser and the server receive
  the same format, so the canonical hash comparison remains the gate; a
  mismatch is still a red state.
- DOCX tabs keep their existing constraint: the server reads the project input
  from disk when `source` is absent. Reading a secondary `.docx` path remains
  out of scope (the browser still previews it from `/api/asset` bytes).

## Consequences

- Opening any `.md`, `.html`, `.tex` or `.docx` tab previews that tab with its
  own parser; the same `DocIR` and layout pipeline runs afterwards.
- The S gate gains **S11**: from a Markdown project, open `extra.html` and
  `extra.tex`, and require a fresh build whose wasm hash equals the native
  verify hash, whose proof text contains the frontend's marker, and which does
  not retain raw `<h1>` / `\section` source.
- `BuildRequest` gains one optional field. Existing JSON without `format`
  deserializes to `None`; no schema version bump is needed because the Studio
  API is not canonical output.
- No engine, template, DocIR, PDF or corpus byte changes.

## Alternatives

- **Replace the project input when a tab is activated.** Would rewrite the
  server's source of truth and break the file watcher and the project payload;
  the preview only needs a per-build format.
- **Add a separate endpoint per frontend.** Duplicates the same build path and
  makes the wasm/native comparison harder to keep aligned.
