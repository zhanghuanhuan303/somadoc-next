# ADR-0063: Word (.docx) reader

- Status: Accepted
- Date: 2026-11-21
- Closes: D24
- Related: N1 (no layout fidelity from foreign formats), ADR-0024 (HTML),
  ADR-0025 (LaTeX), `docs/debt.md` D24
- Touches: `soma-docx` (new crate), `soma-cli` (`--input … .docx`)

## Context

Word documents are the most common source a Chinese author actually has.
DocIR has always been the target of every frontend; a `.docx` frontend was
deferred until there was a real file to test against.

A `.docx` is a zip archive of OOXML parts. The repository has a hard rule of
zero external dependencies in the formatting core, so neither a zip library nor
an XML library could be pulled in.

## Decision

### 1. A new crate with its own reader

`soma-docx` implements, in dependency-free `no_std + alloc` Rust:

- zip: End of Central Directory, Central Directory, local headers, method 0
  (store) and method 8 (deflate), CRC-32 check when the archive declares one;
- inflate: stored, fixed-Huffman and dynamic-Huffman blocks, LZ77 with
  overlapping copies;
- XML: a small tolerant tokenizer (elements, attributes, entities, CDATA,
  comments, BOM);
- WordprocessingML → DocIR.

### 2. What is translated

| OOXML | DocIR |
|---|---|
| `w:p`, `w:pStyle=Heading1…9`/`Title` | `Block::Paragraph`, `Block::Heading` (7–9 clamped to 6) |
| `w:r`, `w:t`, `w:tab`, `w:br` | `Inline::Text`, space, `Inline::LineBreak` |
| `w:b`, `w:i`, `w:u`, `w:vertAlign` | Strong, Emphasis, Underline, Superscript/Subscript |
| `w:hyperlink` | child text (target not resolved: `.rels` is not read) |
| `w:tbl`/`w:tr`/`w:tc`, `w:tblHeader` | `Block::Table`, `Row.header` |
| consecutive `w:numPr` paragraphs | `Block::List` |
| `docProps/core.xml` | `meta.title`, `meta.cover.title`, `meta.authors` |

Unknown elements are skipped; a broken archive is an error with a short reason,
never a panic.

### 3. No fidelity promise

Styles, fonts, colours, indents, page geometry, images, footnotes, fields,
revisions, and merged cells are **not** read. The output is the document's
structure and inline emphasis, to be typeset by a soma template. This is the
same contract as the HTML and LaTeX frontends (N1).

## Consequences

- `soma-cli` chooses the reader by extension; `.docx` is read as bytes, every
  other source as UTF-8 text.
- The crate is `no_std`, but its tests build archives in memory and run under
  `std`.
- Gate A adds `soma-docx` to the dependency allow-list for `soma-cli` and
  checks that it depends only on `soma-doc`.
- A future `.rels` reader would turn `w:hyperlink` into `Inline::Link`; it is
  deliberately out of scope here.
