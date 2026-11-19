//! Real-corpus evaluation and proxy calibration.
//!
//! `corpus/zh/*.md` is a small, self-authored corpus covering technical prose,
//! notices, abstracts, essays, and mixed-script notes. It is real input in the
//! sense that the engine did not generate it, but it is **not** quality-labeled:
//! there are no human preferences attached. This module therefore reports
//! observable page properties and sweeps proxy objectives; it does not claim a
//! quality win.

use std::path::{Path, PathBuf};

use soma_core::{BreakConfig, ParamSet, Permille, Weight, Window};
use soma_doc::Block;
use soma_layout::{layout_with, PaginationStrategy, StyledMetrics, VerticalParams};
use soma_md::parse_markdown;
use soma_metrics::MetricsTable;
use soma_template::{parse_template, resolve};

use crate::score::{measure, PageMetrics, TEMPLATE};

/// One evaluated document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentReport {
    pub name: String,
    pub pages: u32,
    pub paragraphs: u32,
    pub metrics: PageMetrics,
}

/// All documents plus the pooled metrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusReport {
    pub documents: Vec<DocumentReport>,
    pub metrics: PageMetrics,
    pub pages: u32,
    pub paragraphs: u32,
}

impl CorpusReport {
    pub fn csv(&self) -> String {
        let mut out = String::from(
            "document,pages,paragraphs,lines,over_compress,emergency,continuation,widows,orphans,page_free_p95,page_free_max,jitter_p95,bdev_error_p95\n",
        );
        for document in &self.documents {
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                document.name,
                document.pages,
                document.paragraphs,
                document.metrics.lines,
                document.metrics.over_compress_lines,
                document.metrics.emergency_lines,
                document.metrics.continuation_blocks,
                document.metrics.widow_violations,
                document.metrics.orphan_violations,
                document.metrics.page_free_p95(),
                document.metrics.page_free_max(),
                document.metrics.ratio_jitter_p95(),
                document.metrics.bdev_rel_error_p95(),
            ));
        }
        out.push_str(&format!(
            "ALL,{},{},{},{},{},{},{},{},{},{},{},{}\n",
            self.pages,
            self.paragraphs,
            self.metrics.lines,
            self.metrics.over_compress_lines,
            self.metrics.emergency_lines,
            self.metrics.continuation_blocks,
            self.metrics.widow_violations,
            self.metrics.orphan_violations,
            self.metrics.page_free_p95(),
            self.metrics.page_free_max(),
            self.metrics.ratio_jitter_p95(),
            self.metrics.bdev_rel_error_p95(),
        ));
        out
    }
}

/// One point in the tolerance × badness-scale sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepRow {
    pub tolerance: u16,
    pub scale: u32,
    pub over_compress: u32,
    pub emergency: u32,
    pub page_free_p95: i32,
    pub jitter_p95: i32,
    pub pages: u32,
}

impl SweepRow {
    pub fn csv_header() -> &'static str {
        "tolerance,scale,over_compress,emergency,page_free_p95,jitter_p95,pages"
    }

    pub fn csv(&self) -> String {
        format!(
            "{},{},{},{},{},{},{}",
            self.tolerance,
            self.scale,
            self.over_compress,
            self.emergency,
            self.page_free_p95,
            self.jitter_p95,
            self.pages
        )
    }
}

/// Markdown files in `dir`, sorted by file name.
pub fn files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Lay out every document in `dir` with `params`.
pub fn evaluate_dir(dir: &Path, params: &ParamSet) -> Result<CorpusReport, String> {
    let paths = files(dir)?;
    if paths.is_empty() {
        return Err(format!("{}: no .md files", dir.display()));
    }
    let template = parse_template(TEMPLATE).map_err(|error| error.to_string())?;
    let table = MetricsTable::embedded();
    let engine = BreakConfig::new(*params, Window::Paragraph);
    let vertical = VerticalParams::default_v1();

    let mut documents = Vec::with_capacity(paths.len());
    let mut pooled = PageMetrics::default();
    let mut pages = 0u32;
    let mut paragraphs = 0u32;

    for path in paths {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let document = parse_markdown(&source);
        let styled = resolve(&document, &template)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let metrics = StyledMetrics::new(&table, &styled);
        let layout = layout_with(
            &document,
            &styled,
            &metrics,
            &engine,
            PaginationStrategy::VerticalKp,
            vertical,
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;

        let measured = measure(&layout);
        pooled.merge(&measured);
        pages += layout.page_count();
        paragraphs += count_blocks(&document.blocks);
        documents.push(DocumentReport {
            name: path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("document")
                .to_string(),
            pages: layout.page_count(),
            paragraphs: count_blocks(&document.blocks),
            metrics: measured,
        });
    }

    Ok(CorpusReport {
        documents,
        metrics: pooled,
        pages,
        paragraphs,
    })
}

/// Sweep tolerance × badness scale and return rows ordered by the proxy
/// objective: fewer over-compressed lines, then a fuller page, then less
/// line-to-line jitter, then fewer pages.
pub fn calibrate(dir: &Path, tolerances: &[u16], scales: &[u32]) -> Result<Vec<SweepRow>, String> {
    let mut rows = Vec::new();
    for &tolerance in tolerances {
        for &scale in scales {
            let mut params = ParamSet::default_v1();
            params.break_params.tolerance = Permille(tolerance);
            params.break_params.costs.badness_scale = Weight(scale);
            params.validate().map_err(|error| error.to_string())?;
            let report = evaluate_dir(dir, &params)?;
            rows.push(SweepRow {
                tolerance,
                scale,
                over_compress: report.metrics.over_compress_lines,
                emergency: report.metrics.emergency_lines,
                page_free_p95: report.metrics.page_free_p95(),
                jitter_p95: report.metrics.ratio_jitter_p95(),
                pages: report.pages,
            });
        }
    }
    rows.sort_by_key(|row| {
        (
            row.over_compress,
            row.page_free_p95,
            row.jitter_p95,
            row.pages,
            u8::from(row.scale != 100),
        )
    });
    Ok(rows)
}

fn count_blocks(blocks: &[Block]) -> u32 {
    let mut count = 0;
    for block in blocks {
        match block {
            Block::Paragraph(_) | Block::Heading(_) | Block::Section(_) => count += 1,
            Block::List(list) => {
                for item in &list.items {
                    count += count_blocks(&item.blocks);
                }
            }
            Block::BlockQuote(quote) => count += count_blocks(&quote.blocks),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        count += count_blocks(&cell.blocks);
                    }
                }
            }
            Block::Generated(generated) => count += count_blocks(&generated.blocks),
            _ => {}
        }
    }
    count
}

/// Convenience for tests and the CLI: the repository's corpus directory.
pub fn repository_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/zh")
}

/// Parse a comma-separated list of integers.
pub fn parse_list<T: core::str::FromStr>(text: &str) -> Result<Vec<T>, String> {
    text.split(',')
        .map(|item| {
            item.trim()
                .parse::<T>()
                .map_err(|_| format!("bad list item {item:?}"))
        })
        .collect()
}
