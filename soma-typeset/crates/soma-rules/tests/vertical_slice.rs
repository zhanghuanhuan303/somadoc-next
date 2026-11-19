//! End-to-end vertical slice: text -> ParaIR -> greedy / Knuth-Plass ->
//! canonical layout result.

use soma_core::{
    break_paragraph, break_paragraph_greedy, BreakConfig, Breakability, GreedyMode, PunctClass,
    StyleId, SyntheticMetrics, Width,
};
use soma_rules::{justified_style, paragraph_from_lines, paragraph_from_text};

const TEXT: &str = "中文 English，混排";

fn paragraph(width_em: i32) -> soma_core::Paragraph {
    paragraph_from_text(TEXT, justified_style(Width::from_em(width_em)), StyleId(0))
        .expect("fixture text must compile")
}

#[test]
fn end_to_end_break_respects_punctuation_rules() {
    let paragraph = paragraph(7);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let result = break_paragraph(&paragraph, &metrics, &config).unwrap();

    assert_eq!(
        result.lines.last().unwrap().end,
        paragraph.atoms.len() as u32
    );
    assert!(result.lines.len() >= 2);

    // Breaking after "English" would put the Chinese comma at the line start,
    // which the rules layer marks forbidden.
    assert!(result.lines.iter().all(|line| line.end != 3));

    for line in &result.lines {
        let start = paragraph.atoms[line.start as usize].class;
        if line.start > 0 {
            assert!(
                !matches!(start, PunctClass::Close | PunctClass::Middle),
                "line {} starts with forbidden punctuation",
                line.start
            );
        }

        let end = paragraph.atoms[(line.end - 1) as usize].class;
        assert!(
            end != PunctClass::Open,
            "line {} ends with opening punctuation",
            line.end
        );

        assert_eq!(line.boxes.len(), (line.end - line.start) as usize);
        assert_eq!(line.boxes.first().unwrap().atom, line.start);
        assert_eq!(line.boxes.last().unwrap().atom, line.end - 1);

        for window in line.boxes.windows(2) {
            assert!(
                window[0].x.0 < window[1].x.0,
                "atom positions must increase"
            );
        }
    }

    // A justified, non-final line must land exactly on its target width.
    for line in &result.lines {
        if line.end < paragraph.atoms.len() as u32 {
            let last = line.boxes.last().unwrap();
            assert_eq!(
                last.x.0 + last.advance.0,
                line.indent.0 + line.target_width.0
            );
        }
    }
}

#[test]
fn optimal_is_not_worse_than_greedy_baselines() {
    let paragraph = paragraph(7);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let optimal = break_paragraph(&paragraph, &metrics, &config).unwrap();
    let furthest =
        break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::Furthest).unwrap();
    let min_cost =
        break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::MinCost).unwrap();

    assert!(optimal.total_cost <= min_cost.total_cost);
    assert!(optimal.total_cost.0 >= 0);
    assert_eq!(furthest.lines.last().unwrap().end, 6);
}

#[test]
fn trailing_comma_is_trimmed_at_line_end() {
    let paragraph = paragraph_from_text(
        "字字字，",
        justified_style(Width::from_em_frac(7, 2)),
        StyleId(0),
    )
    .unwrap();

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 1);
    let line = &result.lines[0];
    assert_eq!(line.natural_width, Width::from_em(4));
    assert_eq!(line.trim_right, soma_rules::MIDDLE_TRIM_RIGHT);
    assert_eq!(line.trim_left, Width::ZERO);

    let last = line.boxes.last().unwrap();
    assert_eq!(
        last.x.0 + last.advance.0,
        line.indent.0 + line.target_width.0
    );
}

#[test]
fn leading_open_bracket_is_trimmed_at_line_start() {
    let paragraph = paragraph_from_text(
        "（字字字",
        justified_style(Width::from_em_frac(15, 4)),
        StyleId(0),
    )
    .unwrap();

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 1);
    let line = &result.lines[0];
    assert_eq!(line.trim_left, soma_rules::OPEN_TRIM_LEFT);
    assert_eq!(line.trim_right, Width::ZERO);
    assert_eq!(line.boxes[0].x, Width::ZERO - soma_rules::OPEN_TRIM_LEFT);
    assert_eq!(
        line.boxes.last().unwrap().x.0 + line.boxes.last().unwrap().advance.0,
        line.indent.0 + line.target_width.0
    );
}

#[test]
fn markdown_hard_break_is_forced() {
    let paragraph = paragraph_from_lines(
        &["第一行", "第二行"],
        justified_style(Width::from_em(5)),
        StyleId(0),
    )
    .unwrap();

    assert_eq!(paragraph.atoms.len(), 6);
    assert_eq!(paragraph.boundaries.len(), 5);
    assert_eq!(paragraph.boundaries[2].breakable, Breakability::Forced);

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].end, 3);
    assert!(result.lines[0].forced_end);
    assert_eq!(result.lines[1].start, 3);
    assert_eq!(result.lines[1].indent, Width::ZERO);
}

#[test]
fn result_is_byte_identical_across_runs() {
    let paragraph = paragraph(7);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let first = break_paragraph(&paragraph, &metrics, &config).unwrap();
    let second = break_paragraph(&paragraph, &metrics, &config).unwrap();

    assert_eq!(first.canonical_json(), second.canonical_json());
    assert!(first
        .canonical_json()
        .contains("\"schema\":\"soma.layout/6\""));
}
