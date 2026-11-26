# ADR-0101: Studio loads hyphenation patterns and gives them to both builds

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0059 (Liang hyphenation), ADR-0093 (Web Studio), ADR-0100
  (`--host`)
- Touches: `soma-studio` (options, bundle, API), `soma-studio-core`
  (`ProjectPayload`), `soma-studio-ui` (engine, state, API), `docs/studio.md`,
  the Studio gate

## Context

`examples/book/features.md` contains a deliberate stress paragraph: a Latin
word repeated until it is wider than the measure, followed by CLI examples of
index entries. Opening the file in the Studio showed the word running past the
right edge of the page and being clipped. The same paragraph rendered
correctly through `soma render --hyphenation examples/book/hyphen-en.pat`.

The difference was not in the engine. `soma-studio` and
`soma-studio-ui::engine::build` both passed `hyphenation: None` into
`BuildOptions`, while the CLI parsed the pattern file. The Studio therefore
treated every Latin run as one unbreakable atom. Adding a template setting
would be wrong: hyphenation is a language resource, not a design decision, and
the template schema deliberately carries no resource paths.

## Decision

- **`--hyphenation <file>` on `soma-studio`.** The file is parsed as a TeX
  pattern set by the same `soma_hyphen::Patterns` the CLI uses. If the flag is
  omitted, the server looks for `hyphen-en.pat` in the project root (the input
  file's directory); a present file is used and logged, an absent one leaves
  hyphenation off.
- **The patterns travel with the project payload.** `ProjectPayload` gains
  `customHyphenation: bool`, mirroring `customMetrics`. When true the UI fetches
  the bytes once from `/api/hyphenation` (text, UTF-8) and keeps them for every
  build in that tab set.
- **Both builds receive the same text.** Native `native_build` (initial load,
  `/api/verify`, `/api/pdf`) and the wasm `engine::build` take the pattern text
  from the same configured file, so `wasm = native` remains true for documents
  with hyphenated words. The pattern text is not part of the canonical layout;
  only the resulting break decisions are.
- **No core change.** `BuildOptions.hyphenation` already existed and
  `layout_with_hyphenation` is already tested (E26). This ADR only closes the
  Studio plumbing gap.
- The deployment `run.sh` passes the sample patterns explicitly; the project
  root probe is a convenience for projects that already ship
  `hyphen-en.pat`.

## Consequences

- The stress paragraph now breaks inside the long word at pattern points, and
  no line overflows the measure. Chinese text after the word is no longer
  pushed past the page edge.
- Projects with a pattern file produce different line breaks — and therefore
  different canonical layout hashes — than the same project without one. The
  parameter hash is unchanged; `rulesVersion` is unchanged. The corpus baseline
  does not use the Studio and is unaffected.
- `S11` (mixed frontends) still compares wasm and native on a source without
  long Latin words; the new studio-core test covers the hyphenated path.
- The Studio security surface is unchanged: `/api/hyphenation` serves only the
  configured pattern file, like `/api/metrics`. A project cannot point it at an
  arbitrary path.

## Alternatives

- **Read a template key such as `hyphenation: hyphen-en.pat`.** Templates are
  portable design documents; a filesystem path would make them depend on the
  machine and bypass the "template is pure YAML design" rule.
- **Always enable built-in English patterns.** Hyphenation is language-specific
  and changes break decisions; enabling it silently would make every existing
  project's hash drift and would be wrong for non-English Latin text.
- **Only wire the native build.** `/api/verify` would agree with the PDF but not
  with the proof, breaking the central `wasm = native` invariant.
