//! Q′: the effect scorecard.
//!
//! The scorecard is deliberately made of *observable* page properties, not of
//! the cost function's own terms: a page is good when it is full, a paragraph
//! is good when it splits without leaving a single line behind, and a line is
//! good when it does not have to compress past its declared shrink.
//!
//! Two corpora feed it:
//!
//! - `corpus(paragraphs)` builds a deterministic synthetic corpus in code, so
//!   thresholds can be checked without reading files;
//! - [`crate::corpus`] reads `corpus/zh/*.md`, which is real (if small) input.
//!
//! Neither is a quality-labeled corpus. A quality claim still needs Gate Q.

use soma_core::{BreakConfig, ParamSet, Width};
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_layout::{layout_with, PagedLayout, PaginationStrategy, StyledMetrics, VerticalParams};
use soma_metrics::MetricsTable;
use soma_template::{parse_template, resolve};

/// A book-like template: widows/orphans at two lines, stretchable paragraph
/// spacing so the page can close a gap without a hole. Line-end punctuation is
/// trimmed by the breaker (ADR-0036), so no hanging knob is set here.
pub const TEMPLATE: &str = r#"
name: effect-score
page:
  size: A5
  margin: 18mm
  header: 6mm
  footer: 8mm
  header-text: running-title
  folio: "{page}"
table:
  rule-width: 0
  header-rule-width: 0.6pt
  outer-rule-width: 0.8pt
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

/// Build the fixed synthetic corpus. Paragraph lengths vary so page breaking
/// has choices.
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

/// Observable page properties. Vectors keep the raw samples so corpora can be
/// pooled before taking a percentile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageMetrics {
    pub lines: u32,
    pub over_compress_lines: u32,
    pub emergency_lines: u32,
    pub continuation_blocks: u32,
    pub widow_violations: u32,
    pub orphan_violations: u32,
    pub page_free: Vec<i32>,
    pub jitter: Vec<i32>,
    pub bdev_error: Vec<i32>,
}

impl PageMetrics {
    pub fn page_free_p95(&self) -> i32 {
        percentile(&self.page_free, 95)
    }

    pub fn page_free_max(&self) -> i32 {
        self.page_free.iter().copied().max().unwrap_or(0)
    }

    pub fn ratio_jitter_p95(&self) -> i32 {
        percentile(&self.jitter, 95)
    }

    pub fn bdev_rel_error_p95(&self) -> i32 {
        percentile(&self.bdev_error, 95)
    }

    pub fn merge(&mut self, other: &PageMetrics) {
        self.lines += other.lines;
        self.over_compress_lines += other.over_compress_lines;
        self.emergency_lines += other.emergency_lines;
        self.continuation_blocks += other.continuation_blocks;
        self.widow_violations += other.widow_violations;
        self.orphan_violations += other.orphan_violations;
        self.page_free.extend_from_slice(&other.page_free);
        self.jitter.extend_from_slice(&other.jitter);
        self.bdev_error.extend_from_slice(&other.bdev_error);
    }
}

/// Measure one laid-out document.
pub fn measure(optimal: &PagedLayout) -> PageMetrics {
    let mut metrics = PageMetrics::default();

    for page in &optimal.pages {
        for block in &page.blocks {
            if block.line_offset > 0 {
                metrics.continuation_blocks += 1;
            }
            if block.lines_total >= 3 && !block.lines.is_empty() {
                if block.line_offset + 1 == block.lines_total {
                    metrics.widow_violations += 1;
                }
                if block.line_offset == 0 && block.lines.len() == 1 {
                    metrics.orphan_violations += 1;
                }
            }
            let mut previous_ratio: Option<i32> = None;
            for placed in &block.lines {
                metrics.lines += 1;
                if placed.line.overfull {
                    metrics.over_compress_lines += 1;
                }
                if placed.line.emergency {
                    metrics.emergency_lines += 1;
                }
                if let Some(previous) = previous_ratio {
                    metrics
                        .jitter
                        .push((placed.line.ratio_milli - previous).abs());
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
                        metrics.bdev_error.push(error);
                    }
                }
            }
        }
    }

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
        metrics
            .page_free
            .push(free_units * 1000 / line_height.max(1));
    }

    metrics
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scorecard {
    pub paragraphs: u32,
    pub pages: u32,
    pub greedy_pages: u32,
    pub cost_kp: i64,
    pub cost_greedy: i64,
    pub metrics: PageMetrics,
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
            ("lines", self.metrics.lines as i64),
            (
                "over_compress_lines",
                self.metrics.over_compress_lines as i64,
            ),
            ("emergency_lines", self.metrics.emergency_lines as i64),
            (
                "continuation_blocks",
                self.metrics.continuation_blocks as i64,
            ),
            ("widow_violations", self.metrics.widow_violations as i64),
            ("orphan_violations", self.metrics.orphan_violations as i64),
            ("page_free_p95", self.metrics.page_free_p95() as i64),
            ("page_free_max", self.metrics.page_free_max() as i64),
            ("ratio_jitter_p95", self.metrics.ratio_jitter_p95() as i64),
            (
                "bdev_rel_error_p95",
                self.metrics.bdev_rel_error_p95() as i64,
            ),
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
    let engine = BreakConfig::from_params(ParamSet::publish_v1());
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

    Some(Scorecard {
        paragraphs: paragraphs.max(1) as u32,
        pages: optimal.page_count(),
        greedy_pages: greedy.page_count(),
        cost_kp: optimal.pagination_cost,
        cost_greedy: greedy.pagination_cost,
        metrics: measure(&optimal),
    })
}

/// The nearest-rank percentile. Deterministic for a given sample set.
pub fn percentile(values: &[i32], percent: usize) -> i32 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let index = (sorted.len() * percent / 100).min(sorted.len() - 1);
    sorted[index]
}
