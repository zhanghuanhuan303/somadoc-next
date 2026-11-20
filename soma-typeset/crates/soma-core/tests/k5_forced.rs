//! K5 public tests: forced breaks and segment-local line breaking.

use soma_core::{
    break_paragraph, break_paragraph_greedy, Align, Atom, AtomKind, Boundary, BoundaryKind,
    BoundaryRole, BreakConfig, BreakError, Breakability, Decorations, Glue, GreedyMode, Paragraph,
    ParagraphStyle, ParamSet, PunctClass, StyleId, SyntheticMetrics, Width,
};

fn atom(text: &str) -> Atom {
    Atom {
        text: text.into(),
        kind: AtomKind::Cjk,
        class: PunctClass::None,
        style: StyleId(0),
        decor: Decorations::NONE,
        link: None,
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn boundary(breakable: Breakability) -> Boundary {
    Boundary {
        breakable,
        kind: BoundaryKind::Normal,
        role: BoundaryRole::Plain,
        glue: Glue::default(),
        penalty: 0,
    }
}

fn style(width: Width, first_indent: Width) -> ParagraphStyle {
    ParagraphStyle {
        width,
        first_indent,
        hang: Width::ZERO,
        tracking: Width::ZERO,
        line_height: Width::from_em_frac(3, 2),
        align: Align::Justify,
    }
}

/// Four 1em atoms with a forced break after the second one. The paragraph is
/// wide enough for a single perfect line, so only the forced boundary can
/// produce the two-line result.
fn forced_in_the_middle() -> Paragraph {
    Paragraph {
        style: style(Width::from_em(4), Width::ZERO),
        atoms: vec![atom("甲"), atom("乙"), atom("丙"), atom("丁")],
        boundaries: vec![
            boundary(Breakability::Allowed),
            boundary(Breakability::Forced),
            boundary(Breakability::Allowed),
        ],
    }
}

#[test]
fn forced_break_splits_segments() {
    let result = break_paragraph(
        &forced_in_the_middle(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].start, 0);
    assert_eq!(result.lines[0].end, 2);
    assert!(result.lines[0].forced_end);
    assert_eq!(result.lines[0].adjustment, 0);
    assert_eq!(result.lines[1].start, 2);
    assert_eq!(result.lines[1].end, 4);
    assert!(!result.lines[1].forced_end);

    // Two mandatory line costs; the ignored single-line path would cost 10.
    assert_eq!(result.total_cost.0, 20);
}

#[test]
fn forced_segment_no_indent() {
    let paragraph = Paragraph {
        style: style(Width::from_em(4), Width::EM),
        atoms: vec![atom("甲"), atom("乙"), atom("丙")],
        boundaries: vec![
            boundary(Breakability::Forced),
            boundary(Breakability::Allowed),
        ],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].indent, Width::EM);
    assert_eq!(result.lines[0].target_width, Width::from_em(3));
    assert!(result.lines[0].forced_end);

    assert_eq!(result.lines[1].indent, Width::ZERO);
    assert_eq!(result.lines[1].target_width, Width::from_em(4));
    assert_eq!(result.lines[1].start, 1);
}

#[test]
fn forced_end_does_not_stretch_or_short_score() {
    let paragraph = Paragraph {
        style: style(Width::from_em(2), Width::ZERO),
        atoms: vec![atom("甲"), atom("乙")],
        boundaries: vec![boundary(Breakability::Forced)],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    let forced = &result.lines[0];
    assert!(forced.forced_end);
    assert_eq!(forced.natural_width, Width::EM);
    assert_eq!(forced.adjustment, 0);
    assert_eq!(forced.cost.0, 10);
    assert!(!forced.overfull);
    assert!(!forced.emergency);

    // The final segment is still a paragraph last line and pays short_score:
    // (2em - 1em) * 1_000_000 / 2em = 500_000, plus one line cost.
    let last = &result.lines[1];
    assert!(!last.forced_end);
    assert_eq!(last.cost.0, 500_010);
    assert_eq!(result.total_cost.0, 500_020);
}

#[test]
fn consecutive_forced_breaks_create_single_atom_lines() {
    let paragraph = Paragraph {
        style: style(Width::EM, Width::ZERO),
        atoms: vec![atom("甲"), atom("乙"), atom("丙")],
        boundaries: vec![
            boundary(Breakability::Forced),
            boundary(Breakability::Forced),
        ],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 3);
    assert_eq!(result.lines[0].end, 1);
    assert_eq!(result.lines[1].end, 2);
    assert_eq!(result.lines[2].end, 3);
    assert!(result.lines[0].forced_end);
    assert!(result.lines[1].forced_end);
    assert!(!result.lines[2].forced_end);
    assert_eq!(result.lines[0].cost.0, 10);
    assert_eq!(result.lines[1].cost.0, 10);
    // The final one-em line is still a paragraph last line and pays 500_000.
    assert_eq!(result.lines[2].cost.0, 500_010);
    assert_eq!(result.total_cost.0, 500_030);
}

#[test]
fn greedy_respects_forced_breaks() {
    let result = break_paragraph_greedy(
        &forced_in_the_middle(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
        GreedyMode::Furthest,
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].end, 2);
    assert_eq!(result.lines[1].end, 4);
    assert!(result.lines[0].forced_end);
}

#[test]
fn canonical_result_contains_forced_end() {
    let result = break_paragraph(
        &forced_in_the_middle(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();
    let json = result.canonical_json();

    assert!(json.contains("\"schema\":\"soma.layout/6\""));
    assert!(json.contains("\"emergency\":false,\"forcedEnd\":true"));
    assert!(json.contains("\"emergency\":false,\"forcedEnd\":false"));
}

#[test]
fn forced_breaks_disabled_is_rejected() {
    let mut params = ParamSet::default_v1();
    params.features.forced_breaks = false;

    let error = break_paragraph(
        &forced_in_the_middle(),
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(params),
    )
    .unwrap_err();

    assert_eq!(error, BreakError::ForcedBreaksDisabled);
}
