# ADR-0009: Semantic/presentation boundary

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §5.3, ADR-0003, ADR-0004

## Context

Markdown, HTML, and LaTeX mix content with presentation:

- `**bold**`, `<b>`, `\textbf` are semantic emphasis, but also visually bold;
- `<span style="font-size:20px">` is pure presentation;
- a template may want headings in a different font without losing the heading
  semantics.

If content presentation wins, templates cannot provide a uniform design. If
templates ignore semantic nodes, accessibility/search/export loses information.

## Decision

1. **DocIR stores semantics, never physical style.** `Strong`, `Emphasis`,
   `Underline`, `Strike`, `SmallCaps`, `Superscript`, `Subscript`, `Code`,
   `Link`, `FootnoteRef`, etc. are semantic nodes. No font size, family, color,
   margin, or page geometry in DocIR.
2. **Templates decide presentation.** A template rule maps a semantic node or
   role to font/weight/decoration/size. It may restyle a `Strong` as color or
   small caps; it must not silently erase the semantic node.
3. **Presentational content is not authoritative.** HTML `style` attributes,
   layout-oriented classes, and inline CSS do not enter DocIR. Recognized
   semantic tags map to DocIR; everything else becomes `Raw*` with fallback and
   diagnostic. Markdown/LaTeX presentation commands map to semantic nodes when
   recognized.
4. **Missing syntax is allowed.** A front end that cannot express a semantic node
   simply does not emit it. The capability matrix and diagnostics explain the
   gap; another front end can provide it.
5. **Template authority is bounded.** A template cannot write engine knobs
   (tolerance, weights, pagination), and cannot introduce raw content CSS. It
   can define named roles and style rules for DocIR nodes.

## Consequences

- Content is portable across templates without carrying visual overrides.
- The capability matrix becomes a tested contract.
- Raw nodes need a deterministic fallback path in layout/render.
- A future “presentation override” feature requires a new ADR; it must be
  explicit, scoped, and versioned.

## Alternatives rejected

- Content CSS wins: destroys uniform template design and cross-target
  reproducibility.
- Template drops semantics: breaks accessibility, search, and export.
- Store both semantic and physical style in DocIR: two sources of truth and
  unpredictable precedence.
