# ADR-0014: Typography refinements (trim scaling, pair compression, hanging punctuation)

- Status: Accepted
- Date: 2026-11-20
- Related: ADR-0007 (frozen kernel), ADR-0012 (metrics), `docs/proposal-effect-max.md` E3
- Amends: none. The kernel is untouched; all three changes live above it.

## Context

Chinese typesetting is mostly punctuation behaviour. The rules layer already
declared single-character trim caps, but:

- the cap was in root em, so a 20pt heading trimmed exactly as much as 10.5pt
  body text;
- adjacent punctuation (`。」`, `），`, `？！」`) had no pair compression;
- a closing mark at the end of a justified line could only be compressed, never
  allowed to hang into the margin.

## Decision

1. **Scale trim to the atom's own size.** `soma-rules` still emits caps in base
   em (it does not know styles). `soma-layout` rescales every cap by
   `font_size / 1em` after compiling the paragraph, before breaking. At body
   size the values are unchanged.
2. **Pair compression in the rules layer.** A punctuation mark that directly
   follows another punctuation mark adds `PAIR_TRIM` (1/4 em) to its left cap.
   The rule is `trim.pair`, status `ParamOnly`, source clreq §6.3.2.2.
3. **Hanging punctuation as post-processing.** After the kernel returns lines,
   `soma-layout` may move a line's final `Close`/`Middle` mark into the right
   margin by `min(|adjustment|, hanging-cap)`. The line's `adjustment` is reduced
   by the same amount and the last atom box shifts right. It is enabled per
   template (`hanging-punctuation: true`) and never changes breakpoints.

## Consequences

- `PlacedLine.hang` records the shift; `soma.pages/3` writes it. Renderers do not
  need to know about hanging: they paint the boxes as given.
- `hanging-punctuation` off must produce byte-identical output to the previous
  behaviour; a test locks that by comparing breakpoints and hang == 0.
- Hanging relaxes both compression and stretch, because both make justified CJK
  lines look tight or loose.
- The kernel's canonical `soma.layout/6` is untouched: no new kernel field, no
  model-version change.
- The default preset keeps hanging off; `zh-book-v1`-style templates can turn it
  on. This avoids silently changing existing documents.

## Alternatives rejected

- **Hanging inside the kernel.** Would add a break-time dependency on the last
  atom's class and change `ADR-0007`'s frozen cost model for a purely visual
  gain.
- **Global letter-spacing for justification.** Blurs the text and is not how
  Chinese books justify lines.
- **Pair compression in the kernel.** The kernel is script-agnostic by design;
  pair knowledge belongs to the rules layer.
