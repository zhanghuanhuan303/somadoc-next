//! TikZ blocks are display vector objects (ADR-0092).
//!
//! The geometry lives in `soma-tikz`; these tests lock the layout contract:
//! one centred box, paths on the math payload, node labels as math items, and a
//! visible source fallback for syntax outside the subset.

mod common;

use common::{document, engine, metrics, styled, BOOK};
use soma_core::Width;
use soma_doc::block::MathBlock;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, BoxKind, PagedLayout, TextSource};

fn picture(source: &str) -> PagedLayout {
    let doc = document(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        source,
    ))]);
    let styled = styled(&doc, BOOK);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

const FIGURE: &str = "\\begin{tikzpicture}
  \\draw[->] (0,0) -- (2,0);
  \\draw[fill=red!10] (0.2,0.2) rectangle (1.8,1.2);
  \\node[above] at (1,1) {A};
\\end{tikzpicture}";

#[test]
fn a_picture_is_one_centred_vector_box() {
    let layout = picture(FIGURE);
    let block = &layout.pages[0].blocks[0];
    let math = block.math.as_ref().expect("math box");
    assert_eq!(block.kind, BoxKind::Math);
    assert!(!math.paths.is_empty(), "the picture has paths");
    assert!(
        math.items.iter().any(|item| item.text == "A"),
        "the node label is a positioned item"
    );
    assert_eq!(math.baseline, math.height, "the baseline is the box bottom");
    assert_eq!(block.width, math.width);
    assert_eq!(block.height, math.height);
    let content = layout.page.content_width();
    assert_eq!(block.x, Width((content.0 - math.width.0).max(0) / 2));
}

#[test]
fn paths_reach_canonical_json_only_when_present() {
    let tikz = picture(FIGURE).canonical_json();
    assert!(tikz.contains("\"paths\":["), "a picture writes its paths");

    let formula = picture("a^2 + b^2 = c^2").canonical_json();
    assert!(
        !formula.contains("\"paths\""),
        "an ordinary formula keeps the old bytes"
    );
}

#[test]
fn syntax_outside_the_subset_keeps_the_source_visible() {
    let layout =
        picture("\\begin{tikzpicture}\\draw[clip] (0,0) rectangle (1,1);\\end{tikzpicture}");
    let block = &layout.pages[0].blocks[0];
    assert!(block.math.is_none(), "the picture is not laid out");
    let mut text = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            text.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    assert!(
        text.contains("tikzpicture") && text.contains("clip"),
        "the source stays as a text placeholder: {text}"
    );
}

#[test]
fn forced_breaks_never_split_a_picture() {
    let long_text = "\\begin{tikzpicture}\\draw (0,0) grid (6,4);\\end{tikzpicture}";
    let layout = picture(long_text);
    let block = &layout.pages[0].blocks[0];
    assert_eq!(block.kind, BoxKind::Math);
    assert!(block.lines.is_empty(), "a picture is one atomic flow item");
}
