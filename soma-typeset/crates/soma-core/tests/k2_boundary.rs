//! K2 public tests: CjkLatin boundary deviation and trace.

use soma_core::{
    break_paragraph, break_paragraph_traced, Align, Atom, AtomKind, Boundary, BoundaryKind,
    BoundaryRole, BreakConfig, Breakability, Decorations, Glue, Paragraph, ParagraphStyle,
    PunctClass, StyleId, SyntheticMetrics, Width,
};

fn atom(text: &str, kind: AtomKind) -> Atom {
    Atom {
        text: text.into(),
        kind,
        class: PunctClass::None,
        style: StyleId(0),
        decor: Decorations::NONE,
        link: None,
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn cjk_latin_boundary() -> Boundary {
    Boundary {
        breakable: Breakability::Allowed,
        kind: BoundaryKind::Normal,
        role: BoundaryRole::CjkLatin,
        glue: Glue {
            width: Width::from_em_frac(1, 4),
            stretch: Width::from_em_frac(1, 2),
            shrink: Width::from_em_frac(1, 2),
        },
        penalty: 0,
        pair_squeeze: Width::ZERO,
    }
}

fn paragraph() -> Paragraph {
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em_frac(5, 2),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms: vec![
            atom("中", AtomKind::Cjk),
            atom("ab", AtomKind::LatinRun),
            atom("cd", AtomKind::LatinRun),
            atom("ef", AtomKind::LatinRun),
        ],
        boundaries: vec![
            cjk_latin_boundary(),
            cjk_latin_boundary(),
            cjk_latin_boundary(),
        ],
    }
}

#[test]
fn cjk_latin_adjustments_match_the_line_adjustment() {
    let result = break_paragraph(
        &paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2);
    let line = &result.lines[0];
    assert_eq!(line.adjustment, Width::from_em_frac(1, 4).0);

    let sum: i32 = line
        .cjk_latin_adjustments
        .iter()
        .map(|adjust| adjust.adjustment)
        .sum();
    assert_eq!(sum, line.adjustment);
    assert_eq!(line.cjk_latin_adjustments.len(), 1);
    assert_eq!(line.cjk_latin_adjustments[0].boundary, 0);
    assert_eq!(
        line.cjk_latin_adjustments[0].adjustment,
        Width::from_em_frac(1, 4).0
    );
}

#[test]
fn boundary_deviation_uses_the_proportional_assumption() {
    let result = break_paragraph(
        &paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();
    let line = &result.lines[0];

    // Deficit 0.25em, all stretch on one CjkLatin boundary, ideal gap 0.25em:
    // D = 0.25em.
    assert_eq!(line.boundary_deviation, Width::from_em_frac(1, 4).0 as i64);
}

#[test]
fn canonical_result_contains_boundary_evidence() {
    let result = break_paragraph(
        &paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();
    let json = result.canonical_json();

    assert!(json.contains("\"schema\":\"soma.layout/6\""));
    assert!(json.contains("\"boundaryDeviation\":1024"));
    assert!(json.contains("\"cjkLatinAdjustments\":[{\"boundary\":0,\"adjustment\":1024}]"));
}

#[test]
fn trace_explains_each_line_without_changing_layout() {
    let paragraph = paragraph();
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let result = break_paragraph(&paragraph, &metrics, &config).unwrap();
    let (traced_result, trace) = break_paragraph_traced(&paragraph, &metrics, &config).unwrap();

    assert_eq!(result, traced_result);
    assert_eq!(trace.lines.len(), result.lines.len());
    assert!(trace.evaluated_candidates > 0);
    assert!(!trace.fallback_used);
    assert_eq!(trace.tolerance, 3000);

    let line = &trace.lines[0];
    assert_eq!(line.start, 0);
    assert_eq!(line.end, 2);
    assert_eq!(line.ratio_milli, 500);
    assert_eq!(line.boundary_deviation, Width::from_em_frac(1, 4).0 as i64);
    assert_eq!(line.cost, result.lines[0].cost.0);
}

#[test]
fn trace_marks_fallback_lines() {
    let paragraph = Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(2),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms: vec![atom("abcdefghij", AtomKind::LatinRun)],
        boundaries: vec![],
    };

    let (result, trace) = break_paragraph_traced(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert!(trace.fallback_used);
    assert!(trace.lines[0].emergency);
    assert!(trace.lines[0].overfull);
    assert_eq!(trace.lines[0].cost, result.lines[0].cost.0);
}
