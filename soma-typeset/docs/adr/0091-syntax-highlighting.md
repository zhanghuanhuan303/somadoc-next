# ADR-0091: Syntax highlighting is a rule-layer lexer and a template palette

- Status: Accepted
- Date: 2026-11-25
- Related: ADR-0090 (code block panel), ADR-0089 (monospace family),
  ADR-0088 (Plex default), ADR-0009 (semantics/presentation boundary)
- Touches: new crate `soma-highlight`, `soma-core` (`SyntaxClass`),
  `soma-template` (`syntax:` palette), `soma-layout` (`TextEntry.syntax`),
  `soma-render-pdf`, `soma-render-html`, `soma-render-svg`,
  `examples/templates/plex.yaml`, `docs/template.md`,
  `docs/capabilities.md`

## Context

The Rust book code block is not just a gray rectangle: keywords, strings,
comments, numbers and type names carry the mdBook "Rust" highlight palette
(comments `#575757`, keywords `#9d00ec`, strings `#008200`, functions
`#0030f2`, numbers/types `#b21e00`). The engine had no tokenizer at all: a
fence's language tag was retained in DocIR but ignored, and every glyph used
the code block's single style.

Bringing in a general highlighting library would pull a large dependency tree
into a `no_std`, deterministic, cross-target core. It would also make token
colors a property of the library rather than the template, which violates the
semantics/presentation boundary: a book template and a screen template may
want different palettes for the same token class.

## Decision

### Lexer

A new workspace crate, `soma-highlight`, is a dependency-free
`no_std + alloc` line lexer. It exposes:

- `SyntaxClass` lives in `soma-core` so rules, layout and renderers share one
  vocabulary. The classes are: comment, keyword, string, number, type,
  function, variable, meta, literal.
- `Lexer::new(Option<&str>)` selects a language profile. `Lexer::line(&str)`
  returns character-indexed spans and carries the state needed across lines
  (block comments, Rust nested comments, Python triple strings, shell quote
  continuation, HTML comments).
- Recognition is rule-based, not a full parser: comments, strings/char/raw
  strings, escapes, numbers, identifiers, keyword/type/literal sets,
  function-call shape (`name(`), Rust macros (`name!`), attributes and
  preprocessor lines. It is deliberately conservative; a token it does not
  recognize stays the code block's default color.

Supported fences (aliases in parentheses):

| Family | Tags |
|---|---|
| Systems | `rust` (`rs`), `c`, `cpp` (`c++`, `cxx`, `cc`, `h`, `hpp`) |
| Scripts | `shell` (`sh`, `bash`, `zsh`, `console`), `python` (`py`) |
| Documents | `markdown` (`md`), `latex` (`tex`) |
| Data/config | `json`, `yaml` (`yml`), `toml` |
| Web | `html` (`htm`, `xml`), `css` |
| Other | `sql`, `go`, `java`, `kotlin` (`kt`), `swift`, `csharp` (`cs`), `javascript` (`js`), `typescript` (`ts`) |

An unknown tag, `text`, or an empty info string produces no spans. A fence info
string with attributes (`rust,ignore`, `rust,no_run`, `cpp {.line-numbers}`)
selects the language before the first `,`, space or `{`; the attributes stay
metadata and do not affect layout.

### Template palette

`soma.template/9` gains an optional top-level `syntax:` mapping:

```yaml
syntax:
  comment: "#575757"
  keyword: "#9d00ec"
  string:  "#008200"
  number:  "#b21e00"
  type:    "#b21e00"
  function:"#0030f2"
  variable:"#d70025"
  meta:    "#b21e00"
  literal: "#b21e00"
```

Colors are template data. The canonical template writes the section only when
non-empty; a template without it keeps its historical bytes. A missing class
means "use the code block's own color", so a template can color only comments
and keywords if it wants.

The Plex reference transcribes mdBook's Rust theme into this mapping.

### Layout and renderers

`code_item` runs the lexer only when the template has a non-empty palette.
Each atom carries `Option<SyntaxClass>` in `TextEntry`; the atom's style still
supplies face and size. Renderers resolve the class through the style table's
palette and use it only as a fill/color override:

- PDF: `set_fill(class color)` for the glyph run.
- SVG: `fill` on the `<text>`.
- HTML: an inline `color` on the atom span.

The canonical page result does not include token classes: they are derived
from the source line and the template, and the geometry hash is unaffected by
color choice. Layout hash changes only when panel padding changes geometry.

## Consequences

- Highlighting is deterministic, allocation-bounded and available in the wasm
  slice; no new external dependency and no font involvement.
- The same document can be rendered with a dark palette, a print palette, or
  no palette at all by selecting a template; DocIR is unchanged.
- A language not in the table renders as plain code with the panel. Adding a
  language is a new profile in `soma-highlight` with tests, not a core change.
- First-line attributes on a fence are ignored for layout but no longer leak
  into the `lang` string used for selection.
- The lexer is line-oriented; a construct it cannot classify is left default,
  never dropped or reordered. Classified characters appear in at most one span;
  unclassified characters simply keep the code block's color.

## Alternatives

- **Embed syntect/tree-sitter.** Large dependency trees, C/alloc assumptions
  and a different determinism story; rejected for the core.
- **Color in DocIR.** A document would then carry presentation and could not be
  restyled; rejected (ADR-0009).
- **Put colors in the engine config.** Colors are design, not engine knobs;
  rejected.
- **Full language parsers.** They are larger, slower and fail on snippets that
  are not complete translation units. A conservative lexer is the right size
  for typeset snippets.

## Alignment

- `crates/soma-highlight/` — lexer, profiles, tests.
- `crates/soma-core/src/ir.rs` — `SyntaxClass` (shared vocabulary only).
- `crates/soma-template/src/template.rs`, `style.rs`, `canonical.rs` —
  `syntax:` palette and style-table accessor.
- `crates/soma-layout/src/lib.rs` — lexer call and `TextEntry.syntax`.
- `crates/soma-render-*/src/lib.rs` — color override.
- `examples/templates/plex.yaml` — mdBook Rust palette.
- `docs/template.md`, `docs/capabilities.md` — user-facing description.
