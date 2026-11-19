//! The blind-test pipeline, end to end, on the committed dry-run fixture.
//!
//! The fixture is **simulated**: it exists to prove that the kit key, the CSV
//! parser, the Wilson interval, Fleiss' kappa, and the decision rule run in one
//! chain. It is not human data and never counts as a quality claim (D01).

use soma_eval::{apply_key, blind_report, parse_key, parse_observations, BlindDecision};

const PREFERENCES: &str = include_str!("../../../corpus/blind/dry-run/simulated-preferences.csv");
const KEY: &str = include_str!("../../../corpus/blind/kit/key.csv");

#[test]
fn the_dry_run_fixture_runs_through_the_whole_pipeline() {
    let mut observations = parse_observations(PREFERENCES).expect("parse preferences");
    assert_eq!(observations.len(), 150, "30 paragraphs x 5 evaluators");

    let key = parse_key(KEY).expect("parse key");
    assert_eq!(key.len(), 30);
    apply_key(&mut observations, &key).expect("apply key");

    let report = blind_report(&observations).expect("report");
    assert_eq!(report.paragraphs, 30);
    assert_eq!(report.evaluators, 5);
    assert_eq!(report.a_wins + report.b_wins + report.ties, 30);
    assert!(report.sample_ok);
    assert!(
        report.wilson_low < report.point && report.point < report.wilson_high,
        "the interval brackets the point estimate"
    );
    // The fixture is deliberately not a win: the decision must stay honest.
    assert_eq!(report.decision, BlindDecision::Inconclusive);
}

#[test]
fn the_fixture_is_marked_simulated() {
    let first = PREFERENCES.lines().next().expect("a header line");
    assert!(
        first.starts_with('#') && first.contains("SIMULATED"),
        "the fixture must announce that it is not human data: {first}"
    );
}

#[test]
fn dropping_the_key_changes_the_result() {
    let mut raw = parse_observations(PREFERENCES).expect("parse preferences");
    let baseline = blind_report(&raw).expect("report");

    let key = parse_key(KEY).expect("parse key");
    apply_key(&mut raw, &key).expect("apply key");
    let decoded = blind_report(&raw).expect("report");

    assert_ne!(
        baseline.a_wins, decoded.a_wins,
        "the key must actually swap sides"
    );
}
