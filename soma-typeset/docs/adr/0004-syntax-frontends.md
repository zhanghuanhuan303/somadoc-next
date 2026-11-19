# ADR-0004: Syntax frontends and semantic whitelist

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §6

## Context

The engine must ingest Markdown, HTML, LaTeX, and later Word without letting any
one syntax define the document model. HTML/CSS input also creates a temptation
to let presentational CSS control layout, which would bypass the template.

The architecture review accepted:

- Markdown + LaTeX first;
- HTML through a semantic whitelist;
- Word import only.

## Decision

Every front end implements:

```rust
pub trait SyntaxFrontend {
    type Ast;
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> CapabilitySet;
    fn parse(&self, source: &Source) -> Result<Self::Ast, Diagnostic>;
    fn to_doc_ir(&self, ast: Self::Ast) -> Result<Document, Diagnostic>;
}
```

- Front-end ASTs are private; only DocIR crosses the boundary.
- `CapabilitySet` declares supported semantics (underline, math, footnotes,
  ruby, ...).
- Markdown uses `comrak`; HTML uses `html5ever`/`scraper`; LaTeX uses a
  controlled subset; Word (`docx`) is import-only.

### HTML policy

- Accept semantic elements: headings, paragraphs, lists, emphasis, strong,
  underline, strikethrough, sup/sub, links, images, tables, footnotes, ruby.
- Ignore presentational CSS (`style`, layout-oriented classes, positioning).
- Unknown elements become `RawInline`/`RawBlock` with fallback text and a
  diagnostic; no scripts, no network, no file access.
- HTML is not a browser layout engine; the template remains the only design
  authority.

### LaTeX policy

- Parse a documented subset: sections, emphasis, underline, super/subscript,
  math, footnotes, figures, tables, references.
- Do not implement macro expansion or full TeX.
- Unknown commands become raw nodes with fallback + diagnostic.

### Word policy

- Import paragraph/character styles, headings, lists, footnotes, images, and
  simple tables.
- Map Word styles to DocIR roles/classes; do not reproduce Word layout.
- Export is out of scope.

### Missing semantics

A front end that cannot express a semantic node simply does not emit it.
Missing syntax is not an error; the capability matrix and diagnostics explain
the gap. Another front end (HTML/LaTeX) can supply the missing semantic.

## Consequences

- Adding a syntax is a new crate, not a core change.
- Raw nodes require a fallback path in layout and rendering.
- The capability matrix becomes a tested artifact.
- Presentational HTML cannot bypass templates.

## Alternatives rejected

- One universal parser: impossible across Markdown/HTML/LaTeX/Word.
- Full HTML/CSS input: would duplicate a browser and break template authority.
- Word fidelity: out of scope and would dominate the project.
