//! Real-corpus evaluation: structural claims that must hold on `corpus/zh`.

use soma_core::ParamSet;
use soma_eval::corpus::{
    calibrate, evaluate_all, evaluate_dir, math_document, repository_dir, table_document,
};

#[test]
fn the_corpus_is_present_and_every_document_lays_out() {
    let dir = repository_dir();
    let report = evaluate_dir(&dir, &ParamSet::publish_v1()).expect("evaluate");
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
    let first = evaluate_dir(&dir, &ParamSet::publish_v1()).expect("evaluate");
    let second = evaluate_dir(&dir, &ParamSet::publish_v1()).expect("evaluate");
    assert_eq!(first, second);
    assert_eq!(first.csv(), second.csv());
}

#[test]
fn the_corpus_reports_fitness_jumps() {
    let report = evaluate_all(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    // ADR-0042: the column exists to notice a rise. It was 3 while the embedded
    // metrics table had no ink at all and every line end used the rule
    // constants. With exact punctuation ink (ADR-0073/ADR-0074) a line ending on
    // a full-width mark gives back its real side bearing — 0.82 em for `，`
    // instead of the 0.5 em constant — so those lines sit further into the
    // stretch range and a few adjacent pairs cross a fitness boundary. The
    // aggregate `jitter_p95` is unchanged at 377‰ (one document improved, three
    // rose slightly) and no break moved at all, so the new floor is 8.
    assert!(
        report.metrics.fitness_jumps <= 8,
        "{} class jumps: the corpus grew avoidable ones (ADR-0042)",
        report.metrics.fitness_jumps
    );
    assert_eq!(report.csv().lines().count(), report.documents.len() + 2);
}

#[test]
fn the_corpus_has_no_widows_or_orphans() {
    let report = evaluate_dir(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    assert_eq!(report.metrics.widow_violations, 0);
    assert_eq!(report.metrics.orphan_violations, 0);
}

#[test]
fn compression_past_the_shrink_limit_is_rare() {
    let report = evaluate_dir(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    let budget = (report.metrics.lines / 20).max(1);
    assert!(
        report.metrics.over_compress_lines <= budget,
        "{} of {} lines over-compressed",
        report.metrics.over_compress_lines,
        report.metrics.lines
    );
}

#[test]
fn calibration_finds_a_point_with_little_over_compression() {
    let rows = calibrate(&repository_dir(), &[1_500, 2_000], &[50, 100]).expect("calibrate");
    assert_eq!(rows.len(), 4);
    // Trimming the trailing bearing (ADR-0036) lowers the badness of a
    // slightly overfull line that ends in punctuation, so the model now picks
    // a few such lines: their ink overruns the text edge by less than the
    // bearing they gave up. A handful per corpus is expected; a collapse is
    // not, and `compression_past_the_shrink_limit_is_rare` still holds the 5%
    // budget.
    assert!(
        rows[0].over_compress <= 8,
        "the best point over-compresses {} lines",
        rows[0].over_compress
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
    let report = evaluate_all(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    let names: Vec<&str> = report
        .documents
        .iter()
        .map(|document| document.name.as_str())
        .collect();
    assert!(names.contains(&"classics"), "long-form prose: {names:?}");
    assert!(names.contains(&"footnotes"), "footnote-dense: {names:?}");
    assert!(names.contains(&"table-page"), "table fixture: {names:?}");
    assert!(names.contains(&"math-page"), "math fixture: {names:?}");
    assert!(names.contains(&"book"), "book fixture: {names:?}");
    assert!(report.documents.len() >= 10);
    assert!(report.pages >= 25);

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
    assert!(cells >= 60, "expected one box per cell, got {cells}");
    assert!(!layout.pages.is_empty());
}

#[test]
fn the_math_fixture_keeps_one_box_per_formula() {
    use soma_core::BreakConfig;
    use soma_layout::{layout_with, BoxKind, PaginationStrategy, StyledMetrics, VerticalParams};
    use soma_metrics::MetricsTable;
    use soma_template::{parse_template, resolve};

    let document = math_document();
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

    let formulas = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.kind == BoxKind::Math)
        .count();
    assert_eq!(formulas, 6, "one box per formula");

    // The real formulas must have produced math boxes, not text placeholders.
    let with_math = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.math.is_some())
        .count();
    assert_eq!(with_math, 6, "every fixture formula lays out as math");

    let inline = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .map(|block| block.inline_math.len())
        .sum::<usize>();
    assert_eq!(inline, 2, "both inline formulas became object atoms");
}

#[test]
fn the_book_fixture_is_long_and_stable() {
    let report = evaluate_all(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    let book = report
        .documents
        .iter()
        .find(|document| document.name == "book")
        .expect("book fixture");
    assert!(book.pages >= 15, "book has {} pages", book.pages);
    assert!(book.metrics.lines > 300);

    let again = evaluate_all(&repository_dir(), &ParamSet::publish_v1()).expect("evaluate");
    let second = again
        .documents
        .iter()
        .find(|document| document.name == "book")
        .expect("book fixture");
    assert_eq!(book, second, "the book must lay out identically twice");
}
