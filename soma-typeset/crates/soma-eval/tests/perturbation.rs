//! Precision sensitivity: how often do line breaks change when every glyph
//! advance moves by one unit (1/4096 em)?
//!
//! This is an analysis tool, not a gate. Run it with:
//!
//! ```text
//! cargo test -p soma-eval --test perturbation -- --ignored --nocapture
//! ```

use std::path::Path;

use soma_core::{
    BreakConfig, GlyphMetrics, LineMetrics, Metrics, ParamSet, StyleId, Width, Window,
};
use soma_eval::corpus;
use soma_eval::score::TEMPLATE;
use soma_layout::{layout_with, PagedLayout, PaginationStrategy, StyledMetrics, VerticalParams};
use soma_metrics::MetricsTable;
use soma_template::{parse_template, resolve};

/// How an advance is moved away from the baseline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The control: no change; result must be zero differences.
    Zero,
    /// Every glyph loses one unit (the other end of a truncating round).
    MinusOne,
    /// Every glyph gains one unit (a systematic upper bound).
    PlusOne,
    /// A deterministic per-codepoint choice in {-1, 0, +1}: the realistic
    /// "another equally valid rounding" case.
    Jitter,
}

struct Perturbed<'a> {
    inner: &'a dyn Metrics,
    mode: Mode,
}

impl Perturbed<'_> {
    fn glyph_delta(&self, character: char, style: StyleId) -> i32 {
        match self.mode {
            Mode::Zero => 0,
            Mode::MinusOne => -1,
            Mode::PlusOne => 1,
            Mode::Jitter => {
                let hash = (character as u32)
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add((style.0 as u32).wrapping_mul(40_503));
                (hash % 3) as i32 - 1
            }
        }
    }

    fn text_delta(&self, text: &str, style: StyleId) -> i32 {
        match self.mode {
            Mode::Zero | Mode::MinusOne | Mode::PlusOne => {
                let count = text.chars().count() as i32;
                match self.mode {
                    Mode::Zero => 0,
                    Mode::MinusOne => -count,
                    _ => count,
                }
            }
            Mode::Jitter => text
                .chars()
                .map(|character| self.glyph_delta(character, style))
                .sum(),
        }
    }
}

impl Metrics for Perturbed<'_> {
    fn advance(&self, text: &str, style: StyleId) -> Width {
        Width(self.inner.advance(text, style).0 + self.text_delta(text, style))
    }

    fn line_metrics(&self, style: StyleId) -> LineMetrics {
        self.inner.line_metrics(style)
    }

    fn space_width(&self, style: StyleId) -> Width {
        Width(self.inner.space_width(style).0 + self.text_delta(" ", style))
    }

    fn math_advance(&self, text: &str, style: StyleId) -> Width {
        self.inner.math_advance(text, style)
    }

    fn math_metrics(&self, style: StyleId) -> LineMetrics {
        self.inner.math_metrics(style)
    }

    fn object_advance(&self, text: &str, style: StyleId) -> Width {
        self.inner.object_advance(text, style)
    }

    fn object_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        self.inner.object_metrics(text, style)
    }

    fn kerning(&self, left: &str, right: &str, style: StyleId) -> Width {
        self.inner.kerning(left, right, style)
    }

    fn version(&self) -> u16 {
        self.inner.version()
    }

    fn hash(&self) -> u64 {
        self.inner.hash()
    }
}

/// Per-block break sequence, in page order.
type Breaks = Vec<(u32, Vec<(u32, u32)>)>;

fn breaks(layout: &PagedLayout) -> Breaks {
    let mut out: Breaks = Vec::new();
    for page in &layout.pages {
        for block in &page.blocks {
            let slot = match out.iter_mut().find(|(node, _)| *node == block.node.0) {
                Some(slot) => slot,
                None => {
                    out.push((block.node.0, Vec::new()));
                    let last = out.len() - 1;
                    &mut out[last]
                }
            };
            for placed in &block.lines {
                slot.1.push((placed.line.start, placed.line.end));
            }
        }
    }
    out.sort_by_key(|(node, _)| *node);
    out
}

#[derive(Default)]
struct Difference {
    blocks: usize,
    changed_blocks: usize,
    positions: usize,
    changed_positions: usize,
}

fn compare(base: &Breaks, other: &Breaks) -> Difference {
    let mut report = Difference::default();
    for (node, lines) in base {
        let Some((_, other_lines)) = other.iter().find(|(other, _)| other == node) else {
            report.blocks += 1;
            report.changed_blocks += 1;
            report.positions += lines.len();
            report.changed_positions += lines.len();
            continue;
        };
        report.blocks += 1;
        let positions = lines.len().max(other_lines.len());
        let mut changed = 0;
        for index in 0..positions {
            if lines.get(index) != other_lines.get(index) {
                changed += 1;
            }
        }
        report.positions += positions;
        if changed > 0 {
            report.changed_blocks += 1;
            report.changed_positions += changed;
        }
    }
    report
}

fn documents(dir: &Path) -> Vec<(String, soma_doc::Document)> {
    let mut out = Vec::new();
    for path in corpus::files(dir).expect("corpus files") {
        let source = std::fs::read_to_string(&path).expect("corpus file");
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document")
            .to_string();
        out.push((name, soma_md::parse_markdown(&source)));
    }
    out.push((String::from("table-page"), corpus::table_document()));
    out.push((String::from("math-page"), corpus::math_document()));
    out.push((String::from("book"), corpus::book_document()));
    out
}

fn pct(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

#[test]
#[ignore = "analysis: run with --ignored --nocapture"]
fn perturbation_report() {
    let dir = corpus::repository_dir();
    let documents = documents(&dir);
    let template = parse_template(TEMPLATE).expect("template");
    let table = MetricsTable::embedded();
    let params = ParamSet::publish_v1();
    let engine = BreakConfig::new(params, Window::Paragraph);
    let vertical = VerticalParams::default_v1();

    let modes = [
        ("zero", Mode::Zero),
        ("minus-one", Mode::MinusOne),
        ("plus-one", Mode::PlusOne),
        ("jitter", Mode::Jitter),
    ];

    println!(
        "document,blocks,positions,mode,changed_blocks,changed_positions,changed_block_pct,changed_position_pct,pages,lines,emergency,overfull"
    );
    let mut totals = [
        Difference::default(),
        Difference::default(),
        Difference::default(),
        Difference::default(),
    ];
    let mut total_pages = 0u32;
    for (name, document) in &documents {
        let styled = resolve(document, &template).expect("styles");
        let metrics = StyledMetrics::new(&table, &styled);
        let base_layout = layout_with(
            document,
            &styled,
            &metrics,
            &engine,
            PaginationStrategy::VerticalKp,
            vertical,
        )
        .expect("baseline layout");
        let base = breaks(&base_layout);
        total_pages += base_layout.page_count();
        let mut baseline_report = Difference::default();
        for (index, (label, mode)) in modes.iter().enumerate() {
            let perturbed = Perturbed {
                inner: &metrics,
                mode: *mode,
            };
            let layout = layout_with(
                document,
                &styled,
                &perturbed,
                &engine,
                PaginationStrategy::VerticalKp,
                vertical,
            )
            .expect("perturbed layout");
            let difference = compare(&base, &breaks(&layout));
            if index == 0 {
                baseline_report = Difference {
                    blocks: difference.blocks,
                    changed_blocks: 0,
                    positions: difference.positions,
                    changed_positions: 0,
                };
            }
            totals[index].blocks += difference.blocks;
            totals[index].changed_blocks += difference.changed_blocks;
            totals[index].positions += difference.positions;
            totals[index].changed_positions += difference.changed_positions;
            let lines: usize = layout
                .pages
                .iter()
                .flat_map(|page| page.blocks.iter())
                .map(|block| block.lines.len())
                .sum();
            let emergency: usize = layout
                .pages
                .iter()
                .flat_map(|page| page.blocks.iter())
                .flat_map(|block| block.lines.iter())
                .filter(|placed| placed.line.emergency)
                .count();
            let overfull: usize = layout
                .pages
                .iter()
                .flat_map(|page| page.blocks.iter())
                .flat_map(|block| block.lines.iter())
                .filter(|placed| placed.line.overfull)
                .count();
            println!(
                "{name},{},{},{label},{},{},{:.3},{:.3},{},{lines},{emergency},{overfull}",
                baseline_report.blocks,
                baseline_report.positions,
                difference.changed_blocks,
                difference.changed_positions,
                pct(difference.changed_blocks, difference.blocks),
                pct(difference.changed_positions, difference.positions),
                layout.page_count()
            );
        }
    }
    for (index, (label, _)) in modes.iter().enumerate() {
        let total = &totals[index];
        println!(
            "ALL,{},{},{label},{},{},{:.3},{:.3},{}",
            total.blocks,
            total.positions,
            total.changed_blocks,
            total.changed_positions,
            pct(total.changed_blocks, total.blocks),
            pct(total.changed_positions, total.positions),
            total_pages
        );
    }
}
