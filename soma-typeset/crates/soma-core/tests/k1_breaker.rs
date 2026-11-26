//! K1 breaker behavior tests.
//!
//! Justify and Left are supported. Left keeps the justified breakpoints but
//! places boxes at their natural widths; Center and Right are applied by
//! `soma-layout` after breaking, so the kernel rejects them.

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BreakConfig, BreakError,
    Breakability, Decorations, Glue, Paragraph, ParagraphStyle, PunctClass, StyleId,
    SyntheticMetrics, Width,
};

fn two_atom_paragraph(align: Align) -> Paragraph {
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(10),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align,
        },
        atoms: vec![
            Atom {
                text: "中".into(),
                kind: AtomKind::Cjk,
                class: PunctClass::None,
                style: StyleId(0),
                decor: Decorations::NONE,
                link: None,
                compress_left: Width::ZERO,
                compress_right: Width::ZERO,
            },
            Atom {
                text: "文".into(),
                kind: AtomKind::Cjk,
                class: PunctClass::None,
                style: StyleId(0),
                decor: Decorations::NONE,
                link: None,
                compress_left: Width::ZERO,
                compress_right: Width::ZERO,
            },
        ],
        boundaries: vec![Boundary {
            breakable: Breakability::Allowed,
            kind: BoundaryKind::Normal,
            role: soma_core::BoundaryRole::Plain,
            glue: Glue::default(),
            penalty: 0,
        }],
    }
}

#[test]
fn justify_is_supported() {
    let paragraph = two_atom_paragraph(Align::Justify);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    assert!(break_paragraph(&paragraph, &metrics, &config).is_ok());
}

#[test]
fn left_alignment_places_boxes_without_adjustment() {
    let paragraph = two_atom_paragraph(Align::Left);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let result = break_paragraph(&paragraph, &metrics, &config).expect("left is supported");
    let line = &result.lines[0];
    assert_eq!(line.adjustment, 0);
    assert_eq!(line.boxes[0].x, Width::ZERO);
    assert_eq!(line.boxes[1].x, Width::EM);
    // The breakpoints come from the justified model, so the natural width is
    // the full 2em rather than the 10em target.
    assert_eq!(line.natural_width, Width::from_em(2));
}

#[test]
fn centered_and_right_are_rejected_by_the_kernel() {
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    // Centering and right-alignment are layout-level line shifts; the frozen
    // DP only knows Justify and Left.
    for align in [Align::Center, Align::Right] {
        let paragraph = two_atom_paragraph(align);
        assert_eq!(
            break_paragraph(&paragraph, &metrics, &config).unwrap_err(),
            BreakError::UnsupportedAlign(align)
        );
    }
}

#[test]
fn final_line_penalty_is_zero() {
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let mut with_penalty = two_atom_paragraph(Align::Justify);
    with_penalty.boundaries[0].penalty = 123;

    let mut without_penalty = two_atom_paragraph(Align::Justify);
    without_penalty.boundaries[0].penalty = 0;

    let with_penalty = break_paragraph(&with_penalty, &metrics, &config).unwrap();
    let without_penalty = break_paragraph(&without_penalty, &metrics, &config).unwrap();

    assert_eq!(with_penalty.total_cost, without_penalty.total_cost);
}
