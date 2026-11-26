# ADR-0119: GB2312 subsets and a single Markdown entry document

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0118 (browser-local Studio), the huans somadoc lab's
  `subset-cjk.py`
- Touches: `soma-studio-ui` (`local.rs`, `state.rs`, `selftest.rs`),
  `examples/book/welcome.md`, `scripts/local-assets.sh`, deployment `run.sh`

## Context

ADR-0118 made the public Studio browser-local, but its first PDF export still
had to choose between a document-specific subset and the full 19 MB CJK faces.
That coverage check was both a branch and a worst-case download; the first PDF
could wait for full fonts. The huans somadoc lab solves the same problem with a
fixed **GB2312 levels 1+2** subset (about 1.7 MB per weight), served separately
from the engine and used for both display and PDF. Its `glyph_count` is 7 199
characters once punctuation and the template extras are included.

The public page should also be a single editable document, not a project tree
full of examples. A visitor should land on one Markdown file, change it, and
see the proof and PDF follow.

## Decision

1. **One Markdown entry document.** The embedded source is
   `examples/book/welcome.md`. Local mode shows exactly one file,
   `welcome.md`; opening another local file is not part of this mode. The page
   explains that replacing the prose is the intended workflow.
2. **One font set for proof and PDF.** Plex Regular/Bold are subset to GB2312
   levels 1+2 (plus ASCII, CJK punctuation, full-width forms and a small extras
   set). Mono is a Latin subset; the math face is copied whole because equations
   can reach any glyph. All files live under `/fonts/v2/`.
3. **No coverage check and no fallback.** The same URLs feed `@font-face` and
   `local::pdf`. The browser normally has the bytes cached from first paint, so
   export only parses them and writes the PDF. Characters outside GB2312 render
   as empty boxes in the browser; the native CLI keeps the full faces.
4. **The public bundle shrinks.** `run.sh` points the server's `--font-*`
   arguments at `dist/fonts/v2/`; the duplicated `fonts/` directory, figures and
   the long example book are not part of the public deployment.
5. **S12 measures the export.** The local self-test reports PDF size and
   elapsed milliseconds, so a regression in export latency is visible.

## Consequences

- On the welcome document the first local PDF is about **223 KB in ~50 ms**;
  there is no second font download and no coverage branch.
- First paint loads about 4.4 MB of subset font data (1.6 MB per Plex weight,
  1.2 MB math, 42 KB mono), less than a quarter of the full 19 MB faces.
- Documents that use characters outside GB2312 lose those glyphs in the browser
  preview and PDF. This is the same trade-off as the lab; it is recorded as
  D119. The CLI and server PDF can still use the full faces when configured.
- The public deployment is one source file plus the template/metrics/engine and
  `dist/`; `?server=1` still exposes the file tree for development.
- No model, canonical schema, `modelVersion` or `rulesVersion` change.

## Alternatives considered

- **Keep the document-specific subset plus full-font fallback.** Rejected: the
  fallback is exactly the slow path the user hit, and the coverage check added a
  branch without guaranteeing a fast first export.
- **Ship full 19 MB faces.** Rejected: the lab's measurements and our earlier
  19 MB download show the first paint cost is unacceptable.
- **WOFF2 display subset + TTF PDF subset.** Rejected: the display bytes cannot
  be reused by the PDF writer, so export still downloads a second set and parses
  it; sharing one TTF set is faster and simpler.
- **Generate a per-document subset in the browser.** Rejected for now: it needs
  a font subsetter in wasm (size, latency) and cannot help the first paint.
