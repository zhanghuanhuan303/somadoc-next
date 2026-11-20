//! K1 real-skeleton benchmark.
//!
//! Run with:
//! `cargo run -p soma-core --example bench --release`
//!
//! This intentionally uses the public breaker seam and the synthetic metrics
//! table; it is a reproducibility check, not a font benchmark.

use std::time::Instant;

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BreakConfig, Breakability,
    Decorations, Glue, Paragraph, ParagraphStyle, ParamSet, PunctClass, StyleId, SyntheticMetrics,
    Width,
};

fn paragraph(atom_count: usize) -> Paragraph {
    let atoms = (0..atom_count)
        .map(|_| Atom {
            text: "中".into(),
            kind: AtomKind::Cjk,
            class: PunctClass::None,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        })
        .collect();

    let boundaries = (0..atom_count.saturating_sub(1))
        .map(|_| Boundary {
            breakable: Breakability::Allowed,
            kind: BoundaryKind::Normal,
            role: soma_core::BoundaryRole::Plain,
            glue: Glue::default(),
            penalty: 0,
        })
        .collect();

    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(20),
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

fn main() {
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    for atom_count in [2_000, 4_000] {
        let paragraph = paragraph(atom_count);
        let start = Instant::now();
        let result = break_paragraph(&paragraph, &metrics, &config).expect("benchmark fixture");
        let elapsed = start.elapsed();

        println!(
            "atoms={atom_count} lines={} elapsed_ms={:.2}",
            result.lines.len(),
            elapsed.as_secs_f64() * 1_000.0
        );
    }

    // Deterministic length policy: the same input above the K-P limit must be
    // laid out by Greedy-C and marked, never by a wall-clock decision.
    let mut params = ParamSet::default_v1();
    params.break_params.max_kp_atoms = Some(2_000);
    let limited_config = BreakConfig::from_params(params);
    let paragraph = paragraph(4_000);
    let start = Instant::now();
    let result = break_paragraph(&paragraph, &metrics, &limited_config).expect("benchmark fixture");
    let elapsed = start.elapsed();

    println!(
        "atoms=4000 max_kp_atoms=2000 lines={} elapsed_ms={:.2} breaker={} length_limited={}",
        result.lines.len(),
        elapsed.as_secs_f64() * 1_000.0,
        result.breaker_used.as_str(),
        result.length_limited
    );
}
