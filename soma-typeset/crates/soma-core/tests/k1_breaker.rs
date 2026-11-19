//! K1 breaker behavior tests.
//!
//! The non-justify case is intentionally red until K1 removes the
//! diagnostic `Align::Left` path from the production breaker.

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BreakConfig, BreakError,
    Breakability, Glue, Paragraph, ParagraphStyle, PunctClass, StyleId, SyntheticMetrics, Width,
};

fn two_atom_paragraph(align: Align) -> Paragraph {
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(10),
            first_indent: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align,
        },
        atoms: vec![
            Atom {
                text: "中".into(),
                kind: AtomKind::Cjk,
                class: PunctClass::None,
                style: StyleId(0),
                compress_left: Width::ZERO,
                compress_right: Width::ZERO,
            },
            Atom {
                text: "文".into(),
                kind: AtomKind::Cjk,
                class: PunctClass::None,
                style: StyleId(0),
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
fn non_justify_is_unsupported() {
    let paragraph = two_atom_paragraph(Align::Left);
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let error = break_paragraph(&paragraph, &metrics, &config).unwrap_err();
    assert_eq!(error, BreakError::UnsupportedAlign(Align::Left));
}

#[test]
fn every_non_justify_align_variant_is_unsupported() {
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    // The enum keeps the full set so adding Left/Center/Right later is not a
    // breaking schema change; model v1 only implements Justify.
    for align in [Align::Left, Align::Center, Align::Right] {
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
