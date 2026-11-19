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

Provenance: every file was written for this repository. No third-party text is
included. The corpus is dedicated to the project under CC-BY-4.0, matching the
sample-document policy in `prd.md`.

Rules:

- UTF-8, Markdown subset accepted by `soma-md` (headings, paragraphs, lists,
  emphasis, inline code). No tables, HTML, or nested lists.
- Blank line between blocks. Do not hard-wrap prose: let the engine break lines.
- Keep documents short enough to review by eye, long enough to paginate
  (a few hundred to about a thousand characters each).
- Adding a document is additive; changing one changes the calibration numbers,
  so note the change in `docs/corpus-calibration.md`.
