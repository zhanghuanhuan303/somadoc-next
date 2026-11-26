# ADR-0068: figure units, table headers, and intrinsic image sizes

- Status: Accepted
- Date: 2026-11-21
- Related: ADR-0033 (long-document output), ADR-0058 (lists and hanging
  indents), ADR-0066 (Word images), N1
- Touches: `soma-layout` (pagination, figure/table flow, description items),
  `soma-latex` (description labels), `soma-cli` (image sizes)

## Context

Three defects surfaced while assembling the full-capability sample:

1. A page could end between a figure and its caption. The caption then started
   the next page alone, and the figure page was half empty.
2. A table whose header row landed at the foot of a page left the header alone
   there; the body began on the next page with a repeated header.
3. LaTeX and Markdown images were laid out with a fixed 2:3 aspect heuristic.
   A 2:1 PNG therefore occupied more vertical space than it should, and the
   `width` option of `\includegraphics` could not change that because DocIR
   carries no physical width.

Two smaller typographic fixes came with the same pass: a description term is a
run-in label and must not print a bullet, and the index title starts its own
page so its running head does not repeat the title.

## Decision

### 1. Figure and caption are one unit

When a figure has a caption, the figure's flow item is marked
`keep_with_next = true`. The vertical K-P may not end a page after it, so the
pair moves together. A page can still end after the caption.

### 2. A table header never stands alone

`pagination::push_item` marks the first part of a table with header rows and
more parts as `can_break = false`. The header therefore travels with the first
body band; continuation pages still repeat it.

### 3. Intrinsic image sizes are read by the host

The CLI reads a local image's header (PNG IHDR, JPEG SOF) before layout and
fills `ImageRef.intrinsic`. DocIR still has no width field: an image's pixel
size is a fact about the asset, like Word's `wp:extent` (ADR-0066). A missing
or unsupported file keeps the layout's aspect fallback.

`\includegraphics[width=…]` remains unsupported: a physical width is style,
and the template decides the measure. The option is accepted and ignored.

### 4. Description labels are run-in paragraphs

`description` items carry `Attrs.role = "description"`; the layout prints no
marker and suppresses the hanging indent for that item. The LaTeX frontend puts
an ideographic space between the bold term and its definition.

## Consequences

- Existing documents with local images change size (their true aspect ratio is
  now used). This is a correction, not a tuning change: the aspect comes from
  the file, not from a constant.
- Documents without figures or descriptions are byte-identical.
- Two new pagination unit tests pin the figure/caption and header-alone rules.
