# ADR-0088: The Plex design is the built-in default template

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0069 (LaTeX article defaults, superseded), ADR-0081 (Plex
  reference template), ADR-0087 (natural tables), ADR-0090 (code block panel),
  ADR-0091 (syntax palette)
- Touches: `crates/soma-cli/src/main.rs`, `examples/templates/plex.yaml`,
  `examples/templates/latex-article.yaml`, `docs/cli.md`, `README.md`,
  `docs/adr/README.md`, `crates/soma-cli/tests/cli.rs`

## Context

ADR-0069 made LaTeX `article` the built-in template so that `soma render`
worked before a designer wrote a template. ADR-0081 transcribed the pandoc
reference design into `examples/templates/plex.yaml`, but only the explicit
`--template` path used it, so the "default" output no longer matched the
reference page.

The project now has a complete reference design: A4, one-inch margins, IBM
Plex Sans SC, 10 pt on a 14.4 pt baseline, no paragraph indent, a 20 pt
paragraph skip, natural-width booktabs tables, a cover when the document has
metadata, a table of contents, an index, and the Rust-book code block (ADR-0090).

## Decision

The built-in template is `examples/templates/plex.yaml`, embedded with
`include_str!`. `--template` still overrides it. There is one source of truth:
the file the sample script renders is the file the CLI embeds.

The former built-in is archived verbatim as
`examples/templates/latex-article.yaml`. It remains a complete, selectable
template; it is no longer implicit. ADR-0069 is marked superseded, not deleted.

The archive keeps the old design's behaviour:

- A4, 10 pt on a 12 pt baseline, 121.3 x 210.2 mm text block.
- 1.5 em first-line indent, no paragraph skip.
- LaTeX `article` heading sizes, centred short captions, `arabic-dotted`
  heading numbers.
- No code panel and no syntax palette (ADR-0090/0091 are opt-in presentation).

The CLI help and `docs/cli.md` describe the new default. The default-template
test asserts the Plex page geometry, the sans body family and the Plex heading
scheme instead of the article text block.

## Consequences

- A bare `soma render` now produces the reference design, including a cover
  when the document has a title/author and a TOC when it has headings.
- Documents without metadata/headings do not gain empty generated pages: the
  cover generator already skips empty content, and the TOC generator now also
  skips a document with no headings, figures or tables instead of emitting a
  lone "Contents" page.
- Explicit `--template examples/templates/latex-article.yaml` reproduces the
  old default byte-for-byte, so existing projects can pin it.
- `examples/book/book.yaml` is unchanged; it was always explicit.
- The CLI binary embeds one more YAML file; this is a compile-time
  `include_str!`, not a runtime file lookup.

## Alternatives

- **Keep article as default and only add code styling.** It leaves the
  reference design unreachable without a path, which is the opposite of what
  the reference was built for; rejected.
- **Copy the Plex YAML into `main.rs`.** Two copies drift, and the embedded
  template would stop being the one the sample proves; rejected.
- **Make the template a runtime asset next to the binary.** It breaks the
  single-binary deployment and makes the default depend on the working
  directory; rejected.
- **Drop the article template.** Projects pinned to it would lose a
  reproducible layout; rejected. Archive instead.

## Alignment

- `crates/soma-cli/src/main.rs` — `DEFAULT_TEMPLATE` becomes
  `include_str!("../../../examples/templates/plex.yaml")`; help text.
- `examples/templates/plex.yaml` — the built-in design.
- `examples/templates/latex-article.yaml` — the archived old default.
- `crates/soma-cli/tests/cli.rs` — default geometry/family assertion.
- `docs/adr/README.md` — ADR-0069 marked superseded.
- `docs/cli.md`, `README.md` — user-facing default.
