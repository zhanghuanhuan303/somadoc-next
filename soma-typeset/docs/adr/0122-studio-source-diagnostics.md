# ADR-0122: Studio source diagnostics

- Status: Accepted
- Date: 2026-11-28
- Related: ADR-0121 (static browser-local Studio), ADR-0109 (SVG proof)
- Touches: `crates/soma-studio-ui/src/diagnostics.rs`,
  `crates/soma-studio-ui/src/source.rs`, `style/studio.css`, S14,
  `docs/studio.md`

## Context

The Studio showed a build failure only as a banner in the proof pane. The source
pane had no line numbers and no way to point at the offending bytes:

- the parsers are deliberately forgiving, so many authoring mistakes become
  text instead of a hard parse error;
- build errors are strings. Template and engine errors carry a line/column in
  their own source (the YAML), not in the document;
- layout errors carry `node N`, but the UI did not map a node back to a source
  span;
- a `<textarea>` cannot style individual ranges. The editor is a textarea
  because it keeps IME, selection and soft wrapping correct without a
  contenteditable dependency.

The user-facing requirement was stronger than “show the error”: show the
offending line in a panel, underline the exact token with a GitHub-style red
wavy line, and suggest a fix.

## Decision

1. **Add a UI-side diagnostics layer.** `diagnostics::analyze(source, format,
   build_error)` returns `Diagnostic { start, end, line, column, message,
   suggestion, origin }`. It does not change DocIR, the layout result or any
   canonical schema.
2. **Only high-confidence source lints get spans.** The first set is:
   unclosed fenced code block; math parse errors in `$...$` / `$$...$$`
   (unbalanced group/delimiter/environment, unsupported command, missing
   argument); undefined or duplicate footnote definitions; unclosed
   `[[ruby:...]]` / `[[index:...]]` extensions; unmatched LaTeX
   `\begin{...}` / `\end{...}`. Code fences and inline code are masked before
   scanning.
3. **Build errors are appended, not hidden.** When the message names
   `node N`, the pane re-parses the current source, reads its canonical DocIR
   and maps the node id to a byte span. Template/metrics/engine errors still
   appear as a `构建` row with a suggestion, but they do not get a source
   underline because they point at another file.
4. **The panel sits above the editor.** Each row shows `Lline:column` (or
   `构建`), the message and `建议：…`. Clicking a source row selects the span
   and focuses the textarea; clicking a build row with no span shows the
   suggestion as a toast.
5. **The wavy underline is a transparent text mirror.** A `div.source-mirror`
   uses the same font, padding, line-height and wrapping rules as the
   textarea. It renders the source with `color: transparent`; diagnostic
   ranges get `text-decoration: underline wavy` plus a light red background.
   The textarea stays on top with a transparent background, so caret, IME and
   selection keep working. Scroll positions are synchronised.
6. **Cap and deduplicate.** Diagnostics are sorted by line/offset, merged for
   the mirror, deduplicated by span+message and capped at 48 rows. The mirror is
   only mounted when at least one diagnostic has a span.

## Consequences

- The editor gives immediate, location-specific feedback without a new editor
  framework or a schema change.
- The mirror must stay in sync with the textarea's font stack, padding,
  scrollbar width and wrapping. This is a CSS contract; it is locked by S14 and
  a screenshot check, not by a layout algorithm.
- The lints are conservative. Currency `$` is not guessed as math; a lone
  unmatched `$` is not reported. Math is checked only when it is paired and
  parseable by `soma-math`.
- Build errors from template/engine/metrics cannot always be mapped to the
  source; the panel says `构建` and explains where to look instead of drawing a
  misleading underline.
- The mirror duplicates the source text in the DOM while diagnostics exist. It
  is capped and absent when there are no diagnostics; a future worker/editor
  can replace it without changing the diagnostic contract.
- S14 locks the panel, the wavy spans, the suggestion text and click-to-select
  for a sample with an undefined footnote and an unsupported math command.

## Alternatives considered

- **Make the textarea contenteditable and style inline spans.** Rejected:
  IME and selection behavior would become the editor's problem.
- **Embed Monaco/CodeMirror.** Rejected for the MVP: a large dependency for one
  decoration layer, and it would bypass the existing soft-wrap/IME behavior.
- **Return structured errors from every parser and layout pass.** Correct long
  term, but a much larger change across `soma-md`, `soma-latex`, `soma-doc`,
  `soma-template`, `soma-layout` and `soma-math`. The UI layer can add spans
  now; the engine can adopt the same `Diagnostic` shape later.
- **Underline the whole line instead of the token.** Rejected: the request was
  explicitly “具体出错的地方”, and a line background alone hides whether the
  problem is the footnote marker, the command or the closing brace.

## Alignment

The panel and mirror are documented in `docs/studio.md` under “源文”. The S14
gate is part of `scripts/static.sh`; the existing S1–S13 gates are unchanged.
No engine parameter, model version, rule version or canonical schema changes.
