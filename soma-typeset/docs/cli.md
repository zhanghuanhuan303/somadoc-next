# Command line: `soma`

`soma` is the thin I/O shell over the pipeline. It parses one source file,
resolves one template, lays out, and renders. It never re-implements
typesetting: the output is the same canonical result the gates hash.

Design record: ADR-0029.

## Commands

```sh
soma render --input <file|-> [--from md|html|tex|docx] [--template <file>] [--engine <file>] \
            [--format html|svg|json|pdf] [--out <file|dir>] [--metrics <file>] \
            [--font <file[#index]>] [--font-bold <file[#index]>] \
            [--font-sans <file[#index]>] [--font-sans-bold <file[#index]>] \
            [--font-mono <file[#index]>] [--font-mono-bold <file[#index]>] \
            [--font-math <file[#index]>] [--font-fallback <file[#index]>] \
            [--hyphenation <patterns>] [--stats]
soma check  --input <file> [--template <file>] [--engine <file>]
soma --help
soma --version
```

| Flag | Meaning |
|---|---|
| `--input`, `-i` | Source document. `-` reads stdin (requires `--from`); a directory processes all supported files non-recursively and requires `--out <dir>`. Extension picks the frontend: `.md`, `.html`, `.tex`, or `.docx` (read as binary and imported; no layout fidelity). |
| `--from` | Input format override for stdin or when the extension is absent: `md`, `html`, `tex`, or `docx`. |
| `--template`, `-t` | Pure-YAML template. Omitted: the built-in Plex reference — A4, 25.4 mm margins, IBM Plex Sans SC 10 pt on a 14.4 pt baseline, no paragraph indent, a 20 pt paragraph skip, natural-width three-rule tables, a cover when metadata exists, a TOC when headings exist, and the Rust-book code panel (ADR-0088). The former LaTeX `article` default is archived as `examples/templates/latex-article.yaml`. |
| `--engine` | `soma.engine/1` file. Omitted: `latex-v1` defaults (`tolerance=1260`, ragged last line; ADR-0069). |
| `--format`, `-f` | `html` (default), `svg`, `json` (canonical page result), or `pdf`. |
| `--font-fallback` | Optional rescue face for glyphs `--font` lacks. Layout must have measured it: add a `fallback:` face to the metrics table (`tools/metrics-gen --face fallback:normal:<file>`). Without it, missing glyphs render as `.notdef`. |
| `--hyphenation` | TeX pattern file for Liang hyphenation of Latin words. Omitted: no word is broken. |
| `--metrics` | `soma.metrics/7` table; omitted uses the embedded Noto CJK + New Computer Modern Math table. Older versions still load. A version 3+ table carries per-glyph horizontal ink, so layout can land a line end on the ink instead of a class average; version 5 adds the math face's `MATH` constants and per-glyph vertical ink (ADR-0070); version 6 adds the vertical `MathVariants`, so display operators and radicals use the font's design glyphs (ADR-0075); version 7 adds the radical gaps, display fraction gaps and top accent attachments (ADR-0076). |
| `--font` | TrueType font for `--format pdf`; `path#index` selects a TTC face. Required for PDF. |
| `--font-bold`, `--font-math` | Optional bold/math faces. An explicit `--font-math` is trusted as given. Omitted, with a formula in the document, the CLI uses the bundled New Computer Modern Math (the embedded table's face), then probes installed faces; a candidate must match the table's advances (ADR-0077). If none matches, formula symbols use the body face and a warning is printed. |
| `--font-sans`, `--font-sans-bold` | Optional sans family (`font-family: sans` / `hei` / `gothic`). Omitted: fall back to `--font` / `--font-bold`. |
| `--font-mono`, `--font-mono-bold` | Optional monospaced family (`font-family: mono`). Omitted: fall back to the sans face, then the body face (ADR-0089). The metrics table must contain a `mono:` face for layout to use it; `tools/metrics-gen --face mono:normal:<file>` measures one. |
| `--version`, `-V` | Print the package version and exit. |
| `--out`, `-o` | Output path; `-` or omitted writes to stdout. In directory batch mode this must be a directory; each input writes `<stem>.<format-extension>` and collisions are rejected before writing. |
| `--stats` | Print `pages`, `blocks`, `lines`, and the canonical hash to stderr. For PDF, also print the resolved math face: `math: <path> (auto)`, `math: <path> (explicit)`, or `math: none`. |

`tools/metrics-gen` also audits installed fonts (`--audit <dir>`), writes a
fingerprint library (`--fingerprints <csv>`) and recognises an unknown face
against one (`--identify <font> --library <csv>`), checks a face against a
document's character set (`--coverage`) and ranks rescue faces (`--fallback`).
See [`docs/fonts.md`](fonts.md) and [`docs/mixed-font.md`](mixed-font.md).
`scripts/fonts.sh` picks a face and writes the table in one step.

`check` parses and lays out, then prints document facts (`blocks`, `pages`,
`overflow pages`, `non-normal blocks`, `canonical hash`). It writes no output
file.

## Input syntaxes

| Extension | Frontend |
|---|---|
| `.md`, `.markdown` | `soma-md` (documented CommonMark/GFM subset) |
| `.html`, `.htm` | `soma-html` (semantic whitelist) |
| `.tex`, `.latex` | `soma-latex` (documented subset, math source) |
| `.docx` | `soma-docx` (dependency-free zip/OOXML import; no layout fidelity) |

Anything else is an error: the CLI never guesses.

## Examples

```sh
# Markdown to a self-contained HTML page on stdout.
soma render --input note.md > note.html

# LaTeX article with a book template, SVG output.
soma render --input paper.tex --template book.yaml --format svg --out paper.svg

# PDF with an embedded TrueType subset and bookmarks, measured with the same face.
cargo run --manifest-path tools/metrics-gen/Cargo.toml -- \
  --out misans.smt --face serif:normal:/usr/share/fonts/misan/MiSans-Normal.ttf
soma render --input book.tex --template book.yaml --format pdf \
  --font /usr/share/fonts/misan/MiSans-Normal.ttf --metrics misans.smt --out book.pdf

# Markdown from stdin.
cat note.md | soma render --input - --from md > note.html

# Batch: every supported file in a directory, one output per input.
soma render --input drafts/ --out out/ --format pdf \
  --font book.ttf --metrics book.smt

# Canonical JSON for a diff or a golden test.
soma render --input note.md --format json

# Is the document within budget?
soma check --input book.md --template book.yaml
```

## Studio: `soma-studio`

`soma-studio` is the Axum I/O boundary for the web workbench; the UI itself is
`crates/soma-studio-ui` (Leptos CSR, the whole engine in wasm). See
[`docs/studio.md`](studio.md) for the views, URL parameters, font probe, SSE
reload semantics, security headers and the S1–S11 gates.

```sh
cd crates/soma-studio-ui && trunk build --release && cd ../..
cargo run -p soma-studio -- \
  --input examples/book/full.tex \
  --template examples/templates/plex.yaml \
  --metrics examples/templates/out/plex.smt \
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

It accepts the same template, engine, metrics and font flags as `soma`, plus
`--port`, `--dist`, `--open`, `--no-watch`, and `--no-auto-font`. It has no
`--format`: the browser renders the proof from the engine's page fragments, and
PDF export is `POST /api/pdf` (command palette or `⌘K`). The server never
re-breaks the document for the proof.

When font flags are omitted, the server probes fontconfig for Noto Serif/Sans
CJK SC and Noto Sans Mono and accepts a face only if it covers CJK (or `A` for
mono). Auto faces are referenced by family name in the proof and subset into
the PDF, so a missing `--font` does not yield a blank export. `--no-auto-font`
disables this discovery.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Usage, input, template, engine, layout, or write error |

Messages go to stderr and include the path or the failing argument. The CLI
never prints a partial document on error.

## Tests

`crates/soma-cli/tests/cli.rs` runs the real binary: Markdown → HTML, LaTeX math
→ renderer spans, canonical JSON schema, SVG output, a custom template changing
the page width, `--out` file writing, `check`, and the three error paths
(unknown extension, missing input, missing argument).
