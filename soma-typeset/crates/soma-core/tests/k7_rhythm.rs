//! K7: adjacent justified lines may not jump between looseness classes for
//! free (preset `rhythm-v1`).

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BreakConfig, Breakability,
    Decorations, Glue, Paragraph, ParagraphStyle, ParamSet, PunctClass, StyleId, SyntheticMetrics,
    Width,
};

const WIDTH_EM: i32 = 7;

fn word(text: &str) -> Vec<Atom> {
    text.chars()
        .map(|character| Atom {
            text: character.to_string(),
            kind: AtomKind::Cjk,
            class: PunctClass::None,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        })
        .collect()
}

fn space() -> Boundary {
    Boundary {
        breakable: Breakability::Allowed,
        kind: BoundaryKind::Space,
        role: soma_core::BoundaryRole::Plain,
        glue: Glue {
            width: Width::ZERO,
            stretch: Width::from_em_frac(1, 8),
            shrink: Width::from_em_frac(1, 8),
        },
        penalty: 0,
        pair_squeeze: Width::ZERO,
    }
}

/// Words of the given lengths, separated by spaces.
fn glue() -> Boundary {
    Boundary {
        breakable: Breakability::Forbidden,
        kind: BoundaryKind::Normal,
        role: soma_core::BoundaryRole::Plain,
        glue: Glue::default(),
        penalty: 0,
        pair_squeeze: Width::ZERO,
    }
}

fn paragraph(lengths: &[usize]) -> Paragraph {
    let mut atoms = Vec::new();
    let mut boundaries = Vec::new();
    for (index, length) in lengths.iter().enumerate() {
        for (position, atom) in word(&"中".repeat(*length)).into_iter().enumerate() {
            if !atoms.is_empty() {
                // Between words a stretchable space; inside a word a
                // forbidden boundary.
                boundaries.push(if index > 0 && position == 0 {
                    space()
                } else {
                    glue()
                });
            }
            atoms.push(atom);
        }
    }
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(WIDTH_EM),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms,
        boundaries,
    }
}

fn config(params: ParamSet) -> BreakConfig {
    BreakConfig::from_params(params)
}

/// Sum of class jumps between adjacent justified lines.
fn jump_sum(result: &soma_core::LayoutResult) -> i32 {
    let class = |ratio: i32| {
        if ratio < -500 {
            0
        } else if ratio <= 500 {
            1
        } else if ratio <= 1500 {
            2
        } else {
            3
        }
    };
    let mut total = 0;
    let mut previous: Option<i32> = None;
    for line in result
        .lines
        .iter()
        .take(result.lines.len().saturating_sub(1))
    {
        let current = class(line.ratio_milli);
        if let Some(previous) = previous {
            total += (current - previous).abs();
        }
        previous = Some(current);
    }
    total
}

fn compare(lengths: &[usize]) -> (i32, i32) {
    let paragraph = paragraph(lengths);
    let metrics = SyntheticMetrics::default();
    let plain = break_paragraph(&paragraph, &metrics, &config(ParamSet::publish_v1()))
        .expect("plain must break");
    let rhythm = break_paragraph(&paragraph, &metrics, &config(ParamSet::rhythm_v1()))
        .expect("rhythm must break");
    (jump_sum(&plain), jump_sum(&rhythm))
}

#[test]
fn the_preset_is_valid_and_turns_the_feature_on() {
    let params = ParamSet::rhythm_v1();
    params.validate().expect("rhythm-v1 must validate");
    assert!(params.features.fitness);
    assert!(params.break_params.costs.fitness.0 > 0);
    assert_ne!(
        params.param_hash().0,
        ParamSet::publish_v1().param_hash().0,
        "the feature is part of the hash"
    );
}

#[test]
fn the_feature_removes_at_least_one_class_jump() {
    let mut improved = None;
    for first in 2..=5 {
        for second in 1..=4 {
            for third in 1..=5 {
                let lengths = [first, second, third, first, second, second];
                let (plain, rhythm) = compare(&lengths);
                if rhythm < plain {
                    improved = Some((lengths, plain, rhythm));
                    break;
                }
            }
            if improved.is_some() {
                break;
            }
        }
        if improved.is_some() {
            break;
        }
    }
    let (lengths, plain, rhythm) = improved.expect("a fixture with a removable jump");
    assert!(
        rhythm < plain,
        "lengths {lengths:?}: rhythm {rhythm} should be below plain {plain}"
    );
}

#[test]
fn a_zero_weight_is_the_old_solver() {
    let paragraph = paragraph(&[4, 2, 3, 4, 2, 2]);
    let metrics = SyntheticMetrics::default();
    let mut params = ParamSet::rhythm_v1();
    params.break_params.costs.fitness = soma_core::Weight(0);
    let with_flag = break_paragraph(&paragraph, &metrics, &config(params)).expect("breaks");
    let plain =
        break_paragraph(&paragraph, &metrics, &config(ParamSet::publish_v1())).expect("breaks");

    assert_eq!(with_flag.canonical_json(), plain.canonical_json());
}
