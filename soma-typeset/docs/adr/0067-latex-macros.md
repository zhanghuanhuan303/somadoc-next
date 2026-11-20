# ADR-0067: LaTeX macros and counters

- Status: Accepted
- Date: 2026-11-21
- Closes: the macro/counter half of D70
- Related: ADR-0025 (LaTeX subset), `docs/debt.md` D70
- Touches: `soma-latex` (`macros.rs`, `parse_latex`)

## Context

The LaTeX frontend handled a fixed command table. A document that defines one
macro — `\newcommand{\term}{汉字}` — degraded: the definition printed nothing
but every use stayed an unknown command, and counters printed as text. Real
manuscripts define abbreviations and numbering macros constantly.

## Decision

### 1. Expand the source before parsing

`macros::expand` runs after comment stripping and before the block/inline
parser. It is a single left-to-right scan:

- `\newcommand`, `\renewcommand`, `\providecommand` (0–9 braced parameters) and
  `\def\name{body}` register a macro; the definition itself is removed from the
  output;
- `\name` expands with `#1`…`#9` substituted; arguments are grabbed as balanced
  brace groups; `##` becomes a literal `#`;
- expansion recurses with a depth limit of 64. At the limit the call is left as
  source text, never a panic;
- `\newcounter`, `\setcounter`, `\stepcounter`, `\refstepcounter` maintain
  integer counters; `\arabic`, `\roman`, `\Roman`, `\alph`, `\Alph`, `\value`,
  and `\the<name>` print them. `\the<name>` is expanded only for counters this
  document declared, so `\thesection` stays with the template's numbering;
- `verbatim`, `lstlisting`, and `\verb` regions are copied byte for byte.

Unknown commands are copied through, so the existing parser keeps its
"degrade to text" behaviour.

### 2. Deterministic edge cases

- `\alph`/`\Alph` use bijective base 26 (`z`, `aa`, `ab`, …).
- `\roman` covers 1–3999; zero and negative values print as arabic digits.
- A macro call with missing arguments is not expanded.
- Definition commands inside a comment do not register.

## Consequences

- Macros are textual, not semantic: a macro that expands to a block command
  works because expansion happens before block parsing.
- `\newcommand*`, optional/default arguments, parameterised `\def`, `\let`,
  `\newenvironment`, counter auto-reset (`[within]`), and cross-references
  (`\label`/`\ref`, `\refstepcounter` creating an anchor) are not implemented.
- The frontend's capabilities table and `docs/latex.md` record the new subset.
