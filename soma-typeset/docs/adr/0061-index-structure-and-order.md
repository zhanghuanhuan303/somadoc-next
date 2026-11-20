# ADR-0061: index structure and pinyin order

- Status: Accepted
- Date: 2026-11-21
- Closes: D80, D81
- Related: ADR-0033 (long-document output), `docs/debt.md` D80/D81
- Touches: `soma-collate` (new crate), `soma-layout` (`index` module),
  `soma-template` (`index.sort`), `soma-latex`/`soma-md` (grammar unchanged)

## Context

The index was a flat list of strings: code-point order, no hierarchy, no
cross-references, and markers inside table cells or footnote definitions were
discarded. A Chinese book index is normally ordered by pinyin and uses
sub-entries; the code-point order of 汉字 is arbitrary to a reader.

## Decision

### 1. The marker grammar lives in one place

Both frontends already pass the raw marker (`\index{…}` in LaTeX,
`[[index:…]]` in Markdown). `soma-layout::index::parse` owns the grammar:

```text
宋体            main entry
宋体!字面       sub-entry
宋体@songti     sort key (per part)
宋体|see{汉字}  cross-reference
宋体|seealso{…} related reading
```

Escaped `\!`, `\@`, `\|` stay literal. A malformed directive is ignored, never
fatal: an index entry is not worth failing a page over.

### 2. Entries form a tree before they are printed

`index::rows` merges markers that share a path (parents' sort keys + own),
invents a parent line for an orphan sub-entry, sorts paths lexicographically
under the requested collation, and flattens depth-first. `see` suppresses page
numbers; `see also` keeps them.

### 3. Pinyin order is a checked-in table

`soma-collate` holds `pinyin.bin`: 27,584 code points in U+3400–U+4DBF and
U+4E00–U+9FFF, each mapped to one of 1,285 toneless syllabic ranks, generated
by `tools/make-pinyin-table.py` (ICU) and sorted by code point. The crate is
`no_std`, dependency-free, and does a binary search. Ordering is `(not Han,
syllable rank, code point)`, so Latin and digits precede Han and ties are total.

### 4. The template chooses the order

```yaml
index:
  title: 索引
  sort: pinyin      # or codepoint (default)
```

`codepoint` is the default and writes no canonical key, so existing templates
keep their bytes. An unknown value is a template error.

### 5. Markers follow the page that prints them

A marker inside a table cell is attached to the table's flow item; a marker
inside a footnote definition is attached to the item that refers to the note,
because that is the page the note is printed on.

## Consequences

- The index is a generated block like the contents; it still runs the extra
  pass and reports `stable`.
- Indentation is one ideograph of the entry style per level, capped at three.
- Page ranges (`3–5`) are not merged; consecutive pages are listed in full.
- `soma-collate` is a new allowed dependency of `soma-layout` in Gate A.
