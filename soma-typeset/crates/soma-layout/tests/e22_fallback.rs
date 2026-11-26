//! E22: a line that ends on a rescue glyph trims by that face's edge.
//!
//! ADR-0038 made every glyph carry its own bearings; this checks that the
//! breaker reads the bearing of the face that actually draws the atom, not the
//! style's primary face.

mod common;

use common::{document, engine, paragraph, styled};
use soma_core::{Metrics, Width};
use soma_layout::{layout, StyledMetrics};
use soma_metrics::MetricsTable;

/// A version 4 blob with two faces: the body draws only `中`, the rescue face
/// only `龘` (U+9F98). The rescue glyph is deliberately given an unusual right
/// bearing (289 units) so a wrong-face trim is impossible to miss.
fn fallback_table() -> MetricsTable {
    let unit: i32 = 4096;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&soma_metrics::MAGIC.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&(unit as u16).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for (family, codepoint) in [(0u8, 0x4E2Du32), (soma_metrics::FAMILY_FALLBACK, 0x9F98)] {
        bytes.extend_from_slice(&[family, 0, 0, 0]);
        for value in [unit * 4 / 5, unit / 5, 0, unit / 4, unit] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&codepoint.to_le_bytes());
        bytes.extend_from_slice(&codepoint.to_le_bytes());
        bytes.extend_from_slice(&unit.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // Version 4: a varint delta from the previous codepoint. The first
        // entry of a face is its absolute codepoint.
        push_varint(&mut bytes, codepoint);
        bytes.push(6);
        bytes.push(18);
    }
    MetricsTable::parse(&bytes).expect("fixture blob")
}

fn push_varint(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Ten em wide: thirty `龘` characters become three justified lines.
const NARROW: &str = r#"
name: mixed
page:
  size: custom
  width: 45mm
  height: 80mm
  margin: 4mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.5
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
"#;

#[test]
fn a_line_ending_in_a_rescue_glyph_trims_by_that_face() {
    let doc = document(vec![paragraph(&"龘".repeat(30))]);
    let styled = styled(&doc, NARROW);
    let table = fallback_table();
    let metrics = StyledMetrics::new(&table, &styled);
    let result = layout(&doc, &styled, &metrics, &engine()).expect("layout");

    let mut checked = 0;
    for block in &result.pages[0].blocks {
        for (index, placed) in block.lines.iter().enumerate() {
            let line = &placed.line;
            // The last line is ragged: it keeps its advance and is not stretched.
            if index + 1 == block.lines.len() || line.trim_right.0 == 0 {
                continue;
            }
            // 289 is the rescue face's quantised right bearing; the body face
            // has no glyph for 龘 at all.
            assert!(
                (280..=300).contains(&line.trim_right.0),
                "trim {} is not the rescue bearing",
                line.trim_right.0
            );
            let last = line.boxes.last().expect("a line has boxes");
            assert_eq!(
                last.x.0 + last.advance.0,
                line.indent.0 + line.target_width.0,
                "the line end must land on the text edge"
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "expected several justified rescue lines");
}

#[test]
fn only_the_rescue_glyph_is_reported_as_fallback() {
    let doc = document(vec![paragraph("中龘中")]);
    let styled = styled(&doc, NARROW);
    let table = fallback_table();
    let metrics = StyledMetrics::new(&table, &styled);
    let node = match &doc.blocks[0] {
        soma_doc::Block::Paragraph(block) => block.meta.id,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    let style = common::style_of(&styled, node);

    assert!(!metrics.uses_fallback("中", style));
    assert!(metrics.uses_fallback("龘", style));
    assert_eq!(metrics.advance("龘", style), Width(4096));
    assert_eq!(metrics.advance("中", style), Width(4096));
}
