//! Real-corpus evaluation: structural claims that must hold on `corpus/zh`.

use soma_core::ParamSet;
use soma_eval::corpus::{calibrate, evaluate_dir, repository_dir};

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
