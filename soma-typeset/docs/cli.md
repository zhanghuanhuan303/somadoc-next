# Command line: `soma`

`soma` is the thin I/O shell over the pipeline. It parses one source file,
resolves one template, lays out, and renders. It never re-implements
typesetting: the output is the same canonical result the gates hash.

Design record: ADR-0029.

## Commands

```sh
soma render --input <file> [--template <file>] [--engine <file>] \
            [--format html|svg|json|pdf] [--out <file>] [--metrics <file>] \
            [--font <file[#index]>] [--font-bold <file[#index]>] \
            [--font-math <file[#index]>] [--hyphenation <patterns>]
soma check  --input <file> [--template <file>] [--engine <file>]
soma --help
```

| Flag | Meaning |
|---|---|
| `--input`, `-i` | Source document. Extension picks the frontend: `.md`, `.html`, `.tex`, or `.docx` (read as binary and imported; no layout fidelity). |
| `--template`, `-t` | Pure-YAML template. Omitted: a built-in A4 template (20 mm margins, 10.5 pt / 1.6, justified). |
| `--engine` | `soma.engine/1` file. Omitted: `publish-v1` defaults. |
| `--format`, `-f` | `html` (default), `svg`, `json` (canonical page result), or `pdf`. |
| `--font-fallback` | Optional rescue face for glyphs `--font` lacks. Layout must have measured it: add a `fallback:` face to the metrics table (`tools/metrics-gen --face fallback:normal:<file>`). Without it, missing glyphs render as `.notdef`. |
| `--hyphenation` | TeX pattern file for Liang hyphenation of Latin words. Omitted: no word is broken. |
| `--metrics` | `soma.metrics/2` table; omitted uses the embedded Noto table. Version 1 tables still load. A version 2 table built from the PDF font also carries punctuation ink, so layout can land a line end on the ink instead of a class average. |

`tools/metrics-gen` also audits installed fonts (`--audit <dir>`), writes a fingerprint library (`--fingerprints <csv>`) and recognises an unknown face against one (`--identify <font> --library <csv>`), checks a face against a document's character set (`--coverage`) and ranks rescue faces (`--fallback`). See [`docs/fonts.md`](fonts.md) and [`docs/mixed-font.md`](mixed-font.md). `scripts/fonts.sh` picks a face and writes the table in one step.
| `--font` | TrueType font for `--format pdf`; `path#index` selects a TTC face. Required for PDF. |
| `--font-bold`, `--font-math` | Optional bold/math faces; fall back to `--font`. |
| `--out`, `-o` | Output path; `-` or omitted writes to stdout. |
| `--stats` | Print `pages`, `blocks`, `lines`, and the canonical hash to stderr. |

`check` parses and lays out, then prints document facts (`blocks`, `pages`,
`overflow pages`, `non-normal blocks`, `canonical hash`). It writes no output
file.

## Input syntaxes

| Extension | Frontend |
|---|---|
| `.md`, `.markdown` | `soma-md` (documented CommonMark/GFM subset) |
| `.html`, `.htm` | `soma-html` (semantic whitelist) |
| `.tex`, `.latex` | `soma-latex` (documented subset, math source) |

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

# Canonical JSON for a diff or a golden test.
soma render --input note.md --format json

# Is the document within budget?
soma check --input book.md --template book.yaml
```

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
