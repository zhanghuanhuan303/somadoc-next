//! D94: the fallback ladder must not degrade into a full O(n²) scan.
//!
//! The emergency stage relaxes the width window, because it accepts arbitrarily
//! short lines. Its *overfull* direction is still bounded — by the declared
//! `emergency_stretch` when `allow_overfull` is set, and by a full shrink when
//! it is not. Without that bound a narrow measure evaluates every `(i, j)` pair:
//! 1.25M candidates at 2000 atoms, 40 ms in a browser.

use soma_core::{
    break_paragraph, break_paragraph_traced, Align, Atom, AtomKind, Boundary, BoundaryKind,
    BoundaryRole, BreakConfig, Breakability, Decorations, FallbackStage, Glue, Paragraph,
    ParagraphStyle, PunctClass, StyleId, SyntheticMetrics, Width,
};

/// Mixed-script paragraph shaped like the `mixed` benchmark corpus: mostly CJK
/// with Latin runs, and CJK glue that can barely stretch.
fn mixed(count: usize, width: i32) -> Paragraph {
    let mut atoms = Vec::with_capacity(count);
    let mut boundaries = Vec::with_capacity(count.saturating_sub(1));
    let mut state = 0x1234_5678u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for index in 0..count {
        let latin = next() % 10 < 3;
        let text = if latin {
            let run = 2 + (next() % 5) as usize;
            "E".repeat(run)
        } else {
            "中".to_string()
        };
        atoms.push(Atom {
            text,
            kind: if latin {
                AtomKind::LatinRun
            } else {
                AtomKind::Cjk
            },
            class: PunctClass::None,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        });
        if index > 0 {
            let cjk_gap = matches!(atoms[index - 1].kind, AtomKind::Cjk)
                && matches!(atoms[index].kind, AtomKind::Cjk);
            boundaries.push(Boundary {
                breakable: Breakability::Allowed,
                kind: BoundaryKind::Normal,
                role: if cjk_gap {
                    BoundaryRole::Plain
                } else {
                    BoundaryRole::CjkLatin
                },
                glue: Glue {
                    width: if cjk_gap {
                        Width::ZERO
                    } else {
                        Width::from_em_frac(1, 4)
                    },
                    stretch: if cjk_gap {
                        Width::from_em_frac(1, 32)
                    } else {
                        Width::from_em_frac(1, 16)
                    },
                    shrink: if cjk_gap {
                        Width::from_em_frac(1, 32)
                    } else {
                        Width::from_em_frac(1, 16)
                    },
                },
                penalty: 0,
                pair_squeeze: Width::ZERO,
            });
        }
    }
    Paragraph {
        style: ParagraphStyle {
            width: Width(width),
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

#[test]
fn the_emergency_stage_is_not_a_full_quadratic_scan() {
    let paragraph = mixed(2_000, Width::from_em(20).0);
    let (result, trace) = break_paragraph_traced(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("the narrow measure must still lay out");

    assert_eq!(trace.fallback_stage, FallbackStage::Emergency);
    // Before D94 this was 1,255,320 — one candidate for every start below every
    // end. The bound leaves the same layout in place with ~40x fewer.
    assert!(
        trace.evaluated_candidates < 200_000,
        "emergency stage evaluated {} candidates",
        trace.evaluated_candidates
    );
    assert!(!result.lines.is_empty());
    // The plan itself must not need overfull lines once the window is bounded.
    assert!(result.lines.iter().all(|line| !line.overfull));
}

#[test]
fn an_unbreakable_atom_still_gets_a_line() {
    // A single atom wider than the measure: the escape hatch must stay open,
    // otherwise the paragraph would report "no feasible break".
    let mut paragraph = mixed(3, Width::from_em(2).0);
    paragraph.atoms[1].text = "W".repeat(40);
    paragraph.atoms[1].kind = AtomKind::LatinRun;
    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("an overfull atom must still lay out");
    assert!(!result.lines.is_empty());
}
