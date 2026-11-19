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
use soma_doc::block::{Cell, Column, Heading, MathBlock, Paragraph, Row, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};
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
    evaluate_documents(load_files(dir)?, params)
}

/// Files plus the structured fixtures that Markdown cannot express yet.
pub fn evaluate_all(dir: &Path, params: &ParamSet) -> Result<CorpusReport, String> {
    let mut documents = load_files(dir)?;
    documents.push((String::from("table-page"), table_document()));
    documents.push((String::from("math-page"), math_document()));
    documents.push((String::from("book"), book_document()));
    evaluate_documents(documents, params)
}

fn load_files(dir: &Path) -> Result<Vec<(String, Document)>, String> {
    let paths = files(dir)?;
    if paths.is_empty() {
        return Err(format!("{}: no .md files", dir.display()));
    }
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document")
            .to_string();
        documents.push((name, parse_markdown(&source)));
    }
    Ok(documents)
}

fn evaluate_documents(
    documents: Vec<(String, Document)>,
    params: &ParamSet,
) -> Result<CorpusReport, String> {
    let template = parse_template(TEMPLATE).map_err(|error| error.to_string())?;
    let table = MetricsTable::embedded();
    let engine = BreakConfig::new(*params, Window::Paragraph);
    let vertical = VerticalParams::default_v1();

    let mut reports = Vec::with_capacity(documents.len());
    let mut pooled = PageMetrics::default();
    let mut pages = 0u32;
    let mut paragraphs = 0u32;

    for (name, document) in documents {
        let styled = resolve(&document, &template).map_err(|error| format!("{name}: {error}"))?;
        let metrics = StyledMetrics::new(&table, &styled);
        let layout = layout_with(
            &document,
            &styled,
            &metrics,
            &engine,
            PaginationStrategy::VerticalKp,
            vertical,
        )
        .map_err(|error| format!("{name}: {error}"))?;

        let measured = measure(&layout);
        pooled.merge(&measured);
        pages += layout.page_count();
        let blocks = count_blocks(&document.blocks);
        paragraphs += blocks;
        reports.push(DocumentReport {
            name,
            pages: layout.page_count(),
            paragraphs: blocks,
            metrics: measured,
        });
    }

    Ok(CorpusReport {
        documents: reports,
        metrics: pooled,
        pages,
        paragraphs,
    })
}

/// A table taller than a page: exercises cell boxes, colspan widths, and row
/// stacking.
pub fn table_document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text(
            "下面是一张跨页表格，用来检验单元盒、列宽与分页是否稳定。",
        )],
    )));

    let columns = (0..3)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect();
    let mut rows = Vec::new();
    let mut wide = cell("编号与类别");
    wide.colspan = 2;
    rows.push(Row::new(
        NodeMeta::synthesized(),
        true,
        vec![wide, cell("说明")],
    ));
    for row in 1..=40 {
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&format!("{row}")), cell("甲类"), cell("短说明")],
        ));
    }
    document.push_block(Block::Table(Table::new(
        NodeMeta::synthesized(),
        columns,
        rows,
    )));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("表格之后的正文，用来确认分页没有丢失内容。")],
    )));
    document.renumber();
    document
}

/// Formula placeholders. `soma-layout` renders a `MathBlock` as one object atom
/// until a math adapter exists (D52); this fixture measures that path.
pub fn math_document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text(
            "下面是一组公式占位。真正的数学排版由适配器提供，核心只负责宽高与基线。",
        )],
    )));
    for source in [
        "E = mc^2",
        "\\int_0^1 x^2 dx = 1/3",
        "\\sum_{i=1}^n i = n(n+1)/2",
        "\\frac{a+b}{c+d} = \\frac{1}{2}",
    ] {
        document.push_block(Block::MathBlock(MathBlock::new(
            NodeMeta::synthesized(),
            source,
        )));
    }
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("公式之后的正文，用来确认对象盒不影响分页顺序。")],
    )));
    document.renumber();
    document
}

const BOOK_PARAGRAPHS: &[&str] = &[
    "长篇文档的分页与短文档不同：页面数量增加后，孤行、寡行和底部留白都会被放大，任何一处偏差都会在几十页后变成明显的节奏问题。",
    "我们要求每一页的填充都可以解释：要么正文排满，要么脚注占位，要么是章节起始的刻意留白。无法解释的空白一律视为缺陷。",
    "中文标点在长篇里更容易暴露问题。连续标点、行首禁则与行尾悬挂如果只在短样本上验证，到了长篇就会在某一页集中出现。",
    "版面的一致性来自可复现，而不是来自某一次调参的运气。同一份文档在两年后应当排成同一个样子，页码与断点都不应漂移。",
    "跨页段落需要保持语义完整：标记与脚注同页，段首不出现孤行，段末不出现寡行。这些规则在短文档里是细节，在长篇里是底线。",
    "性能也是长篇的一部分。分页窗口是精确剪枝，因此页数增加时耗时应当近似线性，而不是随着候选断点数量爆炸。",
];

/// A long document: thirty chapters, twelve paragraphs each.
pub fn book_document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    for chapter in 1..=30 {
        document.push_block(Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            2,
            vec![text(&format!("第{chapter}章"))],
        )));
        for index in 0..12 {
            let paragraph = BOOK_PARAGRAPHS[(chapter + index) % BOOK_PARAGRAPHS.len()];
            document.push_block(Block::Paragraph(Paragraph::new(
                NodeMeta::synthesized(),
                vec![text(paragraph)],
            )));
        }
    }
    document.renumber();
    document
}

fn cell(value: &str) -> Cell {
    Cell::new(
        NodeMeta::synthesized(),
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text(value)],
        ))],
    )
}

fn text(value: &str) -> soma_doc::Inline {
    Text::new(NodeMeta::synthesized(), value).into()
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
            let report = evaluate_all(dir, &params)?;
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
