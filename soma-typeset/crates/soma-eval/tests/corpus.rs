//! Real-corpus evaluation: structural claims that must hold on `corpus/zh`.

use soma_core::ParamSet;
use soma_eval::corpus::{calibrate, evaluate_all, evaluate_dir, repository_dir, table_document};

#[test]
fn the_corpus_is_present_and_every_document_lays_out() {
    let dir = repository_dir();
    let report = evaluate_dir(&dir, &ParamSet::default_v1()).expect("evaluate");
    assert!(
        report.documents.len() >= 5,
        "the corpus must keep its five genres"
    );
    assert!(report.pages >= 5);
    assert!(report.metrics.lines > 100);
    for document in &report.documents {
        assert!(document.pages >= 1, "{} has no pages", document.name);
        assert!(document.metrics.lines > 0, "{} has no lines", document.name);
        assert!(document.paragraphs > 0, "{} has no blocks", document.name);
    }
}

#[test]
fn corpus_evaluation_is_deterministic() {
    let dir = repository_dir();
    let first = evaluate_dir(&dir, &ParamSet::default_v1()).expect("evaluate");
    let second = evaluate_dir(&dir, &ParamSet::default_v1()).expect("evaluate");
    assert_eq!(first, second);
    assert_eq!(first.csv(), second.csv());
}

#[test]
fn the_corpus_has_no_widows_or_orphans() {
    let report = evaluate_dir(&repository_dir(), &ParamSet::default_v1()).expect("evaluate");
    assert_eq!(report.metrics.widow_violations, 0);
    assert_eq!(report.metrics.orphan_violations, 0);
}

#[test]
fn compression_past_the_shrink_limit_is_rare() {
    let report = evaluate_dir(&repository_dir(), &ParamSet::default_v1()).expect("evaluate");
    let budget = (report.metrics.lines / 20).max(1);
    assert!(
        report.metrics.over_compress_lines <= budget,
        "{} of {} lines over-compressed",
        report.metrics.over_compress_lines,
        report.metrics.lines
    );
}

#[test]
fn calibration_finds_a_point_without_over_compression() {
    let rows = calibrate(&repository_dir(), &[1_500, 2_000], &[50, 100]).expect("calibrate");
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0].over_compress, 0,
        "the best point must not compress past the shrink limit"
    );
    assert!(rows.iter().all(|row| row.pages >= 5));
    // Ordering is by the proxy objective; the header names must stay stable.
    assert_eq!(
        soma_eval::corpus::SweepRow::csv_header(),
        "tolerance,scale,over_compress,emergency,page_free_p95,jitter_p95,pages"
    );
}

#[test]
fn the_expanded_corpus_includes_long_form_footnotes_and_a_table() {
    let report = evaluate_all(&repository_dir(), &ParamSet::default_v1()).expect("evaluate");
    let names: Vec<&str> = report
        .documents
        .iter()
        .map(|document| document.name.as_str())
        .collect();
    assert!(names.contains(&"classics"), "long-form prose: {names:?}");
    assert!(names.contains(&"footnotes"), "footnote-dense: {names:?}");
    assert!(names.contains(&"table-page"), "table fixture: {names:?}");
    assert!(report.documents.len() >= 7);
    assert!(report.pages >= 8);

    let table = report
        .documents
        .iter()
        .find(|document| document.name == "table-page")
        .expect("table page");
    assert!(table.pages >= 1);
    assert!(table.metrics.lines > 0);
}

#[test]
fn the_table_fixture_produces_cell_boxes() {
    use soma_core::BreakConfig;
    use soma_layout::{layout_with, BoxKind, PaginationStrategy, StyledMetrics, VerticalParams};
    use soma_metrics::MetricsTable;
    use soma_template::{parse_template, resolve};

    let document = table_document();
    let template = parse_template(soma_eval::score::TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let layout = layout_with(
        &document,
        &styled,
        &metrics,
        &BreakConfig::default(),
        PaginationStrategy::VerticalKp,
        VerticalParams::default_v1(),
    )
    .expect("layout");

    let cells = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.kind == BoxKind::TableCell)
        .count();
    assert!(cells >= 16, "expected one box per cell, got {cells}");
    assert!(!layout.pages.is_empty());
}
