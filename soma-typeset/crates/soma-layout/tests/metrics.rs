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

#[test]
fn bold_inline_text_uses_the_bold_face() {
    use common::{paragraph_with, text};
    use soma_doc::inline::Container;
    use soma_doc::Inline;

    const BOLD: &str = r#"
name: bold
page:
  size: A4
  margin: 20mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: strong
    declarations:
      weight: bold
"#;
    let strong = Inline::Strong(Container::new(NodeMeta::synthesized(), vec![text("Wide")]));
    let doc = document(vec![paragraph_with(vec![text("A"), strong])]);
    let strong_node = match &doc.blocks[0] {
        Block::Paragraph(paragraph) => paragraph.inlines[1].meta().id,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    let styled = styled(&doc, BOLD);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let regular = table.face(0).expect("serif regular");
    let bold = table.face(2).expect("serif bold");
    let size = styled
        .style(styled.style_of(strong_node).expect("strong style"))
        .expect("computed style")
        .font_size
        .0;

    assert_eq!(
        metrics
            .advance("Wide", styled.style_of(strong_node).unwrap())
            .0,
        scale(bold.advance("Wide"), size)
    );
    assert_ne!(
        metrics
            .advance("Wide", styled.style_of(strong_node).unwrap())
            .0,
        scale(regular.advance("Wide"), size)
    );
}

/// A minimal `soma.metrics/2` blob: one face, two uniform blocks, and one
/// punctuation mark whose ink sits in the left third of its advance.
fn ink_table() -> MetricsTable {
    let unit: i32 = 4096;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&soma_metrics::MAGIC.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&(unit as u16).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    for value in [unit * 4 / 5, unit / 5, 0, unit / 4, unit] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for (start, end) in [(0x4E00u32, 0x9FFFu32), (0xFF00, 0xFFEF)] {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&end.to_le_bytes());
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0xFF0Cu32.to_le_bytes());
    bytes.extend_from_slice(&700i16.to_le_bytes());
    bytes.extend_from_slice(&2900i16.to_le_bytes());
    MetricsTable::parse(&bytes).expect("fixture blob")
}

#[test]
fn ink_right_comes_from_the_outline_and_scales_with_the_size() {
    let table = ink_table();
    let doc = document(vec![paragraph("中，")]);
    let styled = styled(&doc, BOOK);
    let metrics = StyledMetrics::new(&table, &styled);
    let flat = TableMetrics::new(&table, 0, soma_core::Width::EM);

    assert_eq!(flat.ink_right("，", StyleId(0)).map(|w| w.0), Some(2900));
    assert_eq!(flat.ink_right("中", StyleId(0)), None, "no ink for a Han");
    assert_eq!(flat.ink_right("，，", StyleId(0)), None, "one glyph only");

    // The body style is 1em, which is exactly the table's unit.
    let body = styled
        .table()
        .styles()
        .iter()
        .position(|style| style.font_size.0 == 4096)
        .expect("a body style");
    assert_eq!(
        metrics.ink_right("，", StyleId(body as u16)).map(|w| w.0),
        Some(2900)
    );
}

#[test]
fn a_layout_with_outline_data_trims_by_the_real_bearing() {
    let doc = document(vec![paragraph("中，".repeat(40).as_str())]);
    let styled = styled(&doc, BOOK);
    let table = ink_table();
    let result = layout(
        &doc,
        &styled,
        &TableMetrics::new(&table, 0, soma_core::Width::EM),
        &engine(),
    )
    .expect("layout");

    let mut trimmed = 0;
    for block in &result.pages[0].blocks {
        for placed in &block.lines {
            let line = &placed.line;
            if placed.y.0 == 0 && line.trim_right.0 == 0 {
                continue;
            }
            if line.trim_right.0 != 0 {
                assert_eq!(
                    line.trim_right.0, 2900,
                    "the trailing mark trims by its outline, not the class constant"
                );
                trimmed += 1;
            }
        }
    }
    assert!(trimmed > 0, "some justified line must end in the mark");
}
