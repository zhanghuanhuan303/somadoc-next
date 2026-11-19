# Chinese evaluation corpus

Purpose: a small, reproducible corpus for the effect scorecard and the
calibration sweep. It is **not** a quality-labeled corpus and it does not
replace Gate Q's human review.

| file | genre | what it stresses |
|---|---|---|
| `technical.md` | technical note | long paragraphs, headings, emphasis, English terms |
| `notice.md` | administrative notice | lists, dates, numbers, short paragraphs |
| `abstract.md` | paper abstract | dense punctuation, semicolons, mixed terminology |
| `editorial.md` | essay | long sentences, quotes, em dashes, rhetorical rhythm |
| `mixed.md` | mixed-script note | URLs, code spans, hyphenated terms, percentages |
| `classics.md` | classical prose (public domain) | long paragraphs, pagination, quotation |
| `footnotes.md` | footnote-dense note | note area, marker-line coupling |
| `table-page` (built-in) | table fixture | cell boxes, colspan/rowspan, repeated header |
| `math-page` (built-in) | formula placeholders | object boxes, order across pages |
| `book` (built-in) | 30-chapter document | long-run pagination, 810 lines, 30 pages |

Provenance: the prose files were written for this repository; `classics.md`
contains public-domain excerpts (Tao Yuanming, Fan Zhongyan, Han Yu, Ouyang
Xiu, Wang Xizhi, Su Shi) with modern punctuation, not a critical edition. The
table, math, and book fixtures are built in code because `soma-md` does not
parse tables or math yet. The corpus is dedicated to the project under
CC-BY-4.0, matching the sample-document policy in `prd.md`.

Rules:

- UTF-8, Markdown subset accepted by `soma-md` (headings, paragraphs, lists,
  emphasis, inline code). No tables, HTML, or nested lists.
- Blank line between blocks. Do not hard-wrap prose: let the engine break lines.
- Keep documents short enough to review by eye, long enough to paginate
  (a few hundred to about a thousand characters each).
- Adding a document is additive; changing one changes the calibration numbers,
  so note the change in `docs/corpus-calibration.md`.
