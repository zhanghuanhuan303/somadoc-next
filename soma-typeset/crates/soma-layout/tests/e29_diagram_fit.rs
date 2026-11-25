//! A wide flowchart must not shrink its labels into illegibility.
//!
//! The diagram is first laid out with a 16 em label cap; when that needs a
//! heavy page scale, narrower caps are tried and the layout with the largest
//! final label size wins (ADR-0113).

mod common;

use common::{document, engine, metrics, styled, BOOK};
use soma_doc::block::{DiagramBlock, DiagramDialect};
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, PagedLayout};

const CHAIN: &str = r#"flowchart LR
  UI["tactus-ui<br/>Em4096 layout + wgpu"] --> CMD["SPSC command ring"]
  CMD --> ENG["tactus-engine<br/>transport + scheduler"]
  ENG --> AUD["tactus-audio<br/>callback + HAL"]
  AUD --> HAL["platform HAL<br/>CoreAudio/AAudio/WASAPI/ALSA"]
"#;

fn picture(source: &str) -> PagedLayout {
    let doc = document(vec![Block::Diagram(DiagramBlock::new(
        NodeMeta::synthesized(),
        DiagramDialect::Mermaid,
        source,
    ))]);
    let styled = styled(&doc, BOOK);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

#[test]
fn a_wide_flowchart_keeps_its_labels_readable() {
    let layout = picture(CHAIN);
    let block = &layout.pages[0].blocks[0];
    let math = block
        .math
        .as_ref()
        .expect("the diagram must render, not fall back to source");
    let labels: Vec<&soma_math::MathItem> = math
        .items
        .iter()
        .filter(|item| !item.text.trim().is_empty())
        .collect();
    assert!(!labels.is_empty(), "the diagram has node labels");

    // Without the fit search this chain is over 1.6x the measure and would
    // either fall back to source or land at the 60% floor. The chosen layout
    // must keep the labels close to their design size (`BOOK` has no diagram
    // selector, so the label inherits the 10.5 pt document style = 4096).
    let design = 4096;
    let smallest = labels.iter().map(|item| item.size.0).min().unwrap_or(0);
    assert!(
        smallest > design * 8 / 10,
        "label size {smallest} is below 80% of the design size"
    );
    assert!(block.width.0 <= layout.page.content_width().0);
}
