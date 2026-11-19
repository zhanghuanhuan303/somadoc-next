# Command line: `soma`

`soma` is the thin I/O shell over the pipeline. It parses one source file,
resolves one template, lays out, and renders. It never re-implements
typesetting: the output is the same canonical result the gates hash.

Design record: ADR-0029.

## Commands

```sh
soma render --input <file> [--template <file>] [--engine <file>] \
            [--format html|svg|json|pdf] [--out <file>] \
            [--font <file[#index]>] [--font-bold <file[#index]>] \
            [--font-math <file[#index]>]
soma check  --input <file> [--template <file>] [--engine <file>]
soma --help
```

| Flag | Meaning |
|---|---|
| `--input`, `-i` | Source document. Extension picks the frontend. |
| `--template`, `-t` | Pure-YAML template. Omitted: a built-in A4 template (20 mm margins, 10.5 pt / 1.6, justified). |
| `--engine` | `soma.engine/1` file. Omitted: `publish-v1` defaults. |
| `--format`, `-f` | `html` (default), `svg`, `json` (canonical page result), or `pdf`. |
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

# PDF with an embedded TrueType subset and bookmarks.
soma render --input book.tex --template book.yaml --format pdf \
  --font /usr/share/fonts/misan/MiSans-Normal.ttf --out book.pdf

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
