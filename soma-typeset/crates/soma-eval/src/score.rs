//! Q′: the effect scorecard.
//!
//! The scorecard is deliberately made of *observable* page properties, not of
//! the cost function's own terms: a page is good when it is full, a paragraph
//! is good when it splits without leaving a single line behind, and a line is
//! good when it does not have to compress past its declared shrink.
//!
//! The corpus is built in code so the numbers are reproducible and comparable
//! across runs. Real documents arrive later (Gate Q).

use soma_core::{BreakConfig, Width};
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_layout::{layout_with, PagedLayout, PaginationStrategy, StyledMetrics, VerticalParams};
use soma_metrics::MetricsTable;
use soma_template::{parse_template, resolve};

/// A book-like template: hanging punctuation on, widows/orphans at two lines,
/// stretchable paragraph spacing so the page can close a gap without a hole.
pub const TEMPLATE: &str = r#"
name: effect-score
page:
  size: A5
  margin: 18mm
  header: 6mm
  footer: 8mm
  header-text: running-title
  folio: "{page}"
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
      hanging-punctuation: true
      hanging-cap: 0.5em
      widow-lines: 2
      orphan-lines: 2
      space-before-plus: 0.25em
  - selector:
      element: heading
    declarations:
      font-size: 16pt
      space-before: 1.2em
      space-before-plus: 0.6em
      keep-with-next: true
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

const SAMPLES: &[&str] = &[
    "中文排版的核心是把规则变成可调的代价模型，而不是在西文引擎上打补丁。",
    "标点密集：「引号」，（括号）；冒号：逗号，句号。问号？感叹号！省略号……",
    "Mixed English 与数字 123，well-known path/to/file 也要能排，不能靠溢出。",
    "段落很长的时候应该允许跨页，而不是把整段推到下一页然后在版心底部留一大片空白。",
    "行内强调 **加粗** 与 *斜体* 会改变字号与度量，但断行只看宽度，行高由行盒决定。",
];

/// Build the fixed corpus. Paragraph lengths vary so page breaking has choices.
pub fn corpus(paragraphs: usize) -> Document {
    let mut doc = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    for index in 0..paragraphs {
        let unit = SAMPLES[index % SAMPLES.len()];
        let mut value = String::from(unit);
        let copies = if index % 5 == 4 {
            40
        } else if index % 3 == 0 {
            3
        } else {
            1
        };
        for _ in 1..copies {
            value.push_str(unit);
        }
        doc.push_block(Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), value).into()],
        )));
    }
    doc.renumber();
    doc
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scorecard {
    pub paragraphs: u32,
    pub pages: u32,
    pub greedy_pages: u32,
    pub cost_kp: i64,
    pub cost_greedy: i64,
    pub lines: u32,
    pub over_compress_lines: u32,
    pub emergency_lines: u32,
    pub continuation_blocks: u32,
    pub widow_violations: u32,
    pub orphan_violations: u32,
    /// Free space at the bottom of a page (excluding the last), in permille of
    /// the body line height.
    pub page_free_p95: i32,
    pub page_free_max: i32,
    /// P95 of |ratio(i) - ratio(i-1)| within a block, in permille.
    pub ratio_jitter_p95: i32,
    /// P95 relative error between the approximate `boundary_deviation` and the
    /// exact mean adjustment, in permille.
    pub bdev_rel_error_p95: i32,
}

impl Scorecard {
    pub fn csv(&self) -> String {
        let mut out = String::new();
        for (name, value) in [
            ("paragraphs", self.paragraphs as i64),
            ("pages", self.pages as i64),
            ("greedy_pages", self.greedy_pages as i64),
            ("cost_kp", self.cost_kp),
            ("cost_greedy", self.cost_greedy),
            ("lines", self.lines as i64),
            ("over_compress_lines", self.over_compress_lines as i64),
            ("emergency_lines", self.emergency_lines as i64),
            ("continuation_blocks", self.continuation_blocks as i64),
            ("widow_violations", self.widow_violations as i64),
            ("orphan_violations", self.orphan_violations as i64),
            ("page_free_p95", self.page_free_p95 as i64),
            ("page_free_max", self.page_free_max as i64),
            ("ratio_jitter_p95", self.ratio_jitter_p95 as i64),
            ("bdev_rel_error_p95", self.bdev_rel_error_p95 as i64),
        ] {
            out.push_str(name);
            out.push(',');
            out.push_str(&value.to_string());
            out.push('\n');
        }
        out
    }
}

pub fn scorecard(paragraphs: usize) -> Option<Scorecard> {
    let doc = corpus(paragraphs.max(1));
    let template = parse_template(TEMPLATE).ok()?;
    let styled = resolve(&doc, &template).ok()?;
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let engine = BreakConfig::default();
    let params = VerticalParams::default_v1();

    let optimal = layout_with(
        &doc,
        &styled,
        &metrics,
        &engine,
        PaginationStrategy::VerticalKp,
        params,
    )
    .ok()?;
    let greedy = layout_with(
        &doc,
        &styled,
        &metrics,
        &engine,
        PaginationStrategy::Greedy,
        params,
    )
    .ok()?;

    Some(analyze(&optimal, &greedy, paragraphs.max(1)))
}

fn analyze(optimal: &PagedLayout, greedy: &PagedLayout, paragraphs: usize) -> Scorecard {
    let mut lines = 0u32;
    let mut over_compress_lines = 0u32;
    let mut emergency_lines = 0u32;
    let mut continuation_blocks = 0u32;
    let mut widow_violations = 0u32;
    let mut orphan_violations = 0u32;
    let mut ratio_jitter: Vec<i32> = Vec::new();
    let mut bdev_errors: Vec<i32> = Vec::new();

    for page in &optimal.pages {
        for block in &page.blocks {
            if block.line_offset > 0 {
                continuation_blocks += 1;
            }
            if block.lines_total >= 3 && !block.lines.is_empty() {
                if block.line_offset + 1 == block.lines_total {
                    widow_violations += 1;
                }
                if block.line_offset == 0 && block.lines.len() == 1 {
                    orphan_violations += 1;
                }
            }
            let mut previous_ratio: Option<i32> = None;
            for placed in &block.lines {
                lines += 1;
                if placed.line.overfull {
                    over_compress_lines += 1;
                }
                if placed.line.emergency {
                    emergency_lines += 1;
                }
                if let Some(previous) = previous_ratio {
                    ratio_jitter.push((placed.line.ratio_milli - previous).abs());
                }
                previous_ratio = Some(placed.line.ratio_milli);

                if !placed.line.cjk_latin_adjustments.is_empty() {
                    let count = placed.line.cjk_latin_adjustments.len() as i64;
                    let exact = placed
                        .line
                        .cjk_latin_adjustments
                        .iter()
                        .map(|adjust| (adjust.adjustment as i64).abs())
                        .sum::<i64>()
                        / count;
                    let approximate = placed.line.boundary_deviation;
                    if exact > 0 {
                        let error = ((approximate - exact).abs() * 1000 / exact) as i32;
                        bdev_errors.push(error);
                    }
                }
            }
        }
    }

    let mut free: Vec<i32> = Vec::new();
    for page in optimal
        .pages
        .iter()
        .take(optimal.pages.len().saturating_sub(1))
    {
        let line_height = page
            .blocks
            .iter()
            .rev()
            .find_map(|block| block.lines.last())
            .map(|placed| placed.line.height.0)
            .unwrap_or(Width::EM.0);
        let used = page.used_height.0 + page.footnote_height.0;
        let free_units = (optimal.page.content_height().0 - used).max(0);
        free.push(free_units * 1000 / line_height.max(1));
    }

    Scorecard {
        paragraphs: paragraphs as u32,
        pages: optimal.page_count(),
        greedy_pages: greedy.page_count(),
        cost_kp: optimal.pagination_cost,
        cost_greedy: greedy.pagination_cost,
        lines,
        over_compress_lines,
        emergency_lines,
        continuation_blocks,
        widow_violations,
        orphan_violations,
        page_free_p95: p95(&mut free),
        page_free_max: free.iter().copied().max().unwrap_or(0),
        ratio_jitter_p95: p95(&mut ratio_jitter),
        bdev_rel_error_p95: p95(&mut bdev_errors),
    }
}

fn p95(values: &mut [i32]) -> i32 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = (values.len() * 95 / 100).min(values.len() - 1);
    values[index]
}
