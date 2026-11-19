//! Q′: the effect scorecard is deterministic and its structural claims hold.

use soma_eval::score::scorecard;

#[test]
fn the_scorecard_is_deterministic() {
    let first = scorecard(20).expect("scorecard");
    let second = scorecard(20).expect("scorecard");
    assert_eq!(first, second);
    assert_eq!(first.csv(), second.csv());
}

#[test]
fn paragraphs_actually_split_across_pages() {
    let card = scorecard(40).expect("scorecard");
    assert!(card.pages > 2, "the corpus must span pages");
    assert!(
        card.continuation_blocks > 0,
        "long paragraphs must continue across pages"
    );
}

#[test]
fn no_widows_or_orphans() {
    let card = scorecard(40).expect("scorecard");
    assert_eq!(card.widow_violations, 0, "widow violations");
    assert_eq!(card.orphan_violations, 0, "orphan violations");
}

#[test]
fn compression_past_the_shrink_limit_stays_rare() {
    let card = scorecard(40).expect("scorecard");
    let budget = (card.lines / 20).max(1);
    assert!(
        card.over_compress_lines <= budget,
        "{} of {} lines compressed past their shrink limit",
        card.over_compress_lines,
        card.lines
    );
    assert_eq!(
        card.emergency_lines, 0,
        "the corpus should not need emergency"
    );
}

#[test]
fn optimal_pagination_is_not_worse_than_greedy() {
    let card = scorecard(40).expect("scorecard");
    assert!(
        card.pages <= card.greedy_pages,
        "K-P used {} pages, greedy {}",
        card.pages,
        card.greedy_pages
    );
    assert!(
        card.cost_kp <= card.cost_greedy,
        "K-P cost {} exceeds greedy {}",
        card.cost_kp,
        card.cost_greedy
    );
}

#[test]
fn pages_are_filled_to_about_one_line() {
    let card = scorecard(40).expect("scorecard");
    assert!(
        card.page_free_p95 < 1_500,
        "P95 bottom slack is {} permille of a line",
        card.page_free_p95
    );
}

#[test]
fn the_approximate_boundary_deviation_is_tracked() {
    let card = scorecard(40).expect("scorecard");
    assert!(
        card.bdev_rel_error_p95 < 200,
        "P95 relative error {} permille",
        card.bdev_rel_error_p95
    );
}
