//! E1: table-backed metrics and per-style scaling.

mod common;

use common::{document, engine, paragraph, styled, text, BOOK};
use soma_core::{Metrics, StyleId};
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, StyledMetrics, TableMetrics};
use soma_metrics::MetricsTable;

fn scale(value: i32, size: i32) -> i32 {
    (value as i64 * size as i64 / 4096) as i32
}

#[test]
fn advances_and_line_metrics_scale_with_the_resolved_font_size() {
    let doc = document(vec![paragraph("中文与 Latin 混排。")]);
    let styled = styled(&doc, BOOK);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let face = table.face(0).expect("serif face");

    for (index, computed) in styled.table().styles().iter().enumerate() {
        let id = StyleId(index as u16);
        let size = computed.font_size.0;
        assert_eq!(
            metrics.advance("中", id).0,
            scale(face.advance_at('中' as u32), size),
            "style {index} at {size}"
        );
        let line = metrics.line_metrics(id);
        assert_eq!(line.ascent.0, scale(face.ascent, size));
        assert_eq!(line.descent.0, scale(face.descent, size));
        assert_eq!(line.leading.0, scale(face.leading, size));
    }

    assert_eq!(metrics.version(), soma_metrics::VERSION);
    assert_eq!(metrics.hash(), table.hash());
}

#[test]
fn a_larger_style_is_proportionally_wider() {
    let doc = document(vec![
        Block::Heading(Heading::new(NodeMeta::synthesized(), 1, vec![text("标题")])),
        paragraph("正文与标题。"),
    ]);
    let styled = styled(&doc, BOOK);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);

    let mut sizes: Vec<(i32, i32)> = styled
        .table()
        .styles()
        .iter()
        .enumerate()
        .map(|(index, computed)| {
            (
                computed.font_size.0,
                metrics.advance("中", StyleId(index as u16)).0,
            )
        })
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    assert!(sizes.len() >= 2, "the fixture must have two sizes");

    let (small_size, small_advance) = sizes[0];
    let (large_size, large_advance) = sizes[sizes.len() - 1];
    assert!(large_size > small_size);
    assert!(large_advance > small_advance);
    let expected = small_advance as i64 * large_size as i64 / small_size as i64;
    assert!((large_advance as i64 - expected).abs() <= 1);
}

#[test]
fn latin_widths_come_from_the_table_not_from_a_class_rule() {
    let doc = document(vec![paragraph("Wi")]);
    let styled = styled(&doc, BOOK);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let face = table.face(0).expect("serif face");
    let body = StyleId(0);
    let size = styled
        .style(body)
        .map(|style| style.font_size.0)
        .unwrap_or(4096);

    assert_eq!(
        metrics.advance("W", body).0,
        scale(face.advance_at('W' as u32), size)
    );
    assert_ne!(
        metrics.advance("W", body),
        metrics.advance("i", body),
        "proportional Latin must not collapse to one width class"
    );
}

#[test]
fn a_plain_adapter_uses_one_face_and_one_size() {
    let table = MetricsTable::embedded();
    let metrics = TableMetrics::new(&table, 1, soma_core::Width::EM);
    let face = table.face(1).expect("sans face");
    assert_eq!(
        metrics.advance("中", StyleId(0)).0,
        face.advance_at('中' as u32)
    );
    assert_eq!(metrics.line_metrics(StyleId(0)).ascent.0, face.ascent);
    assert_eq!(metrics.hash(), table.hash());
}

#[test]
fn layout_with_real_metrics_is_deterministic() {
    let doc = document(vec![paragraph(
        "中文排版的核心是把规则变成可调的代价模型，而不是在西文引擎上打补丁。",
    )]);
    let styled = styled(&doc, BOOK);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let first = layout(&doc, &styled, &metrics, &engine()).expect("layout");
    let second = layout(&doc, &styled, &metrics, &engine()).expect("layout");
    assert_eq!(first.canonical_json(), second.canonical_json());
    assert_eq!(first.metrics_hash, table.hash());
    assert!(!first.pages[0].blocks.is_empty());
}
