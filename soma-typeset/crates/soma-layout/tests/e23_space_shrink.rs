//! E23: a space may give up a third of its advance, and a line that cannot be
//! justified is not crushed.
//!
//! ADR-0041 / D86: the rule table's `1/8 em` space shrink is font-blind, so a
//! narrow measure used to pull a space from 0.29 em to 0.15 em while the line
//! still claimed to fit. The engine now clamps a space's shrink to a third of
//! its advance and refuses an overfull justified line in the normal stage.

use soma_core::{
    BreakConfig, Metrics, Paragraph, ParagraphStyle, StyleId, SyntheticMetrics, Width,
};
use soma_layout::break_paragraph;
use soma_rules::{justified_style, paragraph_from_text};

/// `SyntheticMetrics`: letters are 1/2 em, a space is 1/4 em.
fn body(width: Width) -> Paragraph {
    let style = ParagraphStyle {
        first_indent: Width::ZERO,
        ..justified_style(width)
    };
    paragraph_from_text("aaaa bbbb cccc dddd eeee", style, StyleId(0)).expect("paragraph")
}

#[test]
fn a_space_never_gives_up_more_than_a_third_of_itself() {
    let metrics = SyntheticMetrics::default();
    // Four words are 8.75 em wide; the measure is 8.45 em. The line would need
    // 0.30 em of shrink, and 3/4 em in three spaces is 0.375 em — but by the
    // space rule only 3 x 0.25/3 = 0.25 em is available, so the break is
    // overfull and the engine must choose two shorter lines instead.
    let paragraph = body(Width::from_em_frac(169, 20));
    let result = break_paragraph(&paragraph, &metrics, &BreakConfig::default()).expect("break");
    assert!(result.lines.len() >= 2, "the overfull line must be refused");
    assert!(
        result.lines.iter().all(|line| !line.overfull),
        "no line may claim to fit while overfull: {:?}",
        result
            .lines
            .iter()
            .map(|line| (line.natural_width, line.target_width, line.overfull))
            .collect::<Vec<_>>()
    );
    let space = metrics.space_width(StyleId(0)).0;
    let floor = space * 2 / 3;
    for line in &result.lines {
        for pair in line.boxes.windows(2) {
            let gap = pair[1].x.0 - (pair[0].x.0 + pair[0].advance.0);
            assert!(
                gap >= floor,
                "a space shrank below two thirds: {gap} < {floor}"
            );
        }
    }
}

#[test]
fn a_word_wider_than_the_measure_still_renders() {
    // One atom cannot be broken, so no clamp can save it: the fallback ladder
    // still has to produce a line, and it says so.
    let style = ParagraphStyle {
        first_indent: Width::ZERO,
        ..justified_style(Width::from_em(6))
    };
    let paragraph = paragraph_from_text("aaaaaaaaaaaaaaaa", style, StyleId(0)).expect("paragraph");
    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("an unbreakable word still lays out");
    assert!(result.lines.iter().all(|line| line.overfull));
}
