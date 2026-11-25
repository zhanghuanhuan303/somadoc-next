//! D78: generated figure and table captions use their own element styles.

mod common;

use common::{document, engine, metrics, paragraph};
use soma_core::Align;
use soma_doc::block::{Cell, Column, Figure, Row, Table};
use soma_doc::inline::{Container, Image};
use soma_doc::meta::ImageRef;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Inline, Text};
use soma_layout::{layout, PagedLayout, TextSource};
use soma_template::{parse_template, resolve, FontWeight, StyledDoc};

fn text(value: &str) -> Inline {
    Text::new(NodeMeta::synthesized(), value).into()
}

const TEMPLATE: &str = r#"
name: captions
page:
  size: custom
  width: 160mm
  height: 220mm
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
  - selector:
      element: figureCaption
    declarations:
      font-size: 8pt
      line-height: 1.25
      align: center
      first-indent: 0
  - selector:
      element: tableCaption
    declarations:
      font-size: 7pt
      line-height: 1.25
      align: left
      first-indent: 0
  - selector:
      element: strong
    declarations:
      weight: bold
"#;

fn build(blocks: Vec<Block>) -> (PagedLayout, StyledDoc) {
    let doc: Document = document(blocks);
    let styled = resolve(&doc, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    (layout, styled)
}

fn block_text(layout: &PagedLayout, block: &soma_layout::PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

fn caption<'a>(layout: &'a PagedLayout, wanted: &str) -> &'a soma_layout::PlacedBlock {
    layout.pages[0]
        .blocks
        .iter()
        .find(|block| block_text(layout, block).starts_with(wanted))
        .expect("caption block")
}

fn figure(caption: Vec<Inline>) -> Block {
    let mut figure = Figure::new(
        NodeMeta::synthesized(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef::new("figure.png").with_intrinsic(40, 20),
        ),
    );
    figure.caption = caption;
    Block::Figure(figure)
}

fn table(caption: Vec<Inline>) -> Block {
    let columns = vec![Column::new(NodeMeta::synthesized(), CellAlign::Left)];
    let row = Row::new(
        NodeMeta::synthesized(),
        false,
        vec![Cell::new(NodeMeta::synthesized(), vec![paragraph("单元")])],
    );
    let mut table = Table::new(NodeMeta::synthesized(), columns, vec![row]);
    table.caption = caption;
    Block::Table(table)
}

#[test]
fn a_figure_without_alt_uses_its_caption() {
    let (layout, _) = build(vec![figure(vec![text("架构")])]);
    let block = layout.pages[0]
        .blocks
        .iter()
        .find(|block| block.kind == soma_layout::BoxKind::Figure)
        .expect("figure block");

    assert_eq!(layout.text.block_alt(block.node), Some("架构"));
}

#[test]
fn figure_and_table_captions_use_their_own_elements() {
    let (layout, styled) = build(vec![figure(vec![text("架构")]), table(vec![text("参数")])]);
    let figure = caption(&layout, "架构");
    let table = caption(&layout, "参数");
    let figure_style = styled.style(figure.style).expect("figure caption style");
    let table_style = styled.style(table.style).expect("table caption style");

    assert_eq!(
        figure.style,
        styled.figure_caption_style(),
        "the caption paragraph uses the generated figureCaption style"
    );
    assert_eq!(table.style, styled.table_caption_style());
    assert!(
        figure_style.font_size.0 > table_style.font_size.0,
        "8pt ({}) > 7pt ({})",
        figure_style.font_size.0,
        table_style.font_size.0
    );
    assert_eq!(figure_style.align, Align::Center);
    assert_eq!(table_style.align, Align::Left);

    // The figure caption is centred, so its first box starts away from zero.
    let figure_x = figure.lines[0].line.boxes[0].x.0;
    assert!(figure_x > 0, "centred caption starts at {figure_x}");
    assert_eq!(table.lines[0].line.boxes[0].x.0, 0);
}

#[test]
fn a_caption_does_not_inherit_the_body_first_indent() {
    let source = r#"
name: indent
page:
  size: custom
  width: 160mm
  height: 220mm
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: figureCaption
    declarations:
      first-indent: 0
"#;
    let doc = document(vec![figure(vec![text("架构")])]);
    let styled = resolve(&doc, &parse_template(source).expect("template")).expect("styles");
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let figure = caption(&layout, "架构");

    assert_eq!(
        figure.lines[0].line.indent.0, 0,
        "the caption element resets the indent"
    );
}

#[test]
fn emphasis_inside_a_caption_keeps_the_caption_base_size() {
    let strong = Container::new(NodeMeta::synthesized(), vec![text("note")]);
    let (layout, styled) = build(vec![figure(vec![text("架构"), Inline::Strong(strong)])]);

    let mut found = None;
    for (node, spans) in layout.text.blocks() {
        for entry in spans {
            if entry.text == "note" {
                found = Some((*node, entry.style));
            }
        }
    }
    let (node, style_id) = found.expect("strong text in the caption");
    let style = styled.style(style_id).expect("resolved style");
    let caption = styled
        .style(styled.figure_caption_style())
        .expect("caption style");

    assert_eq!(
        style.font_size, caption.font_size,
        "emphasis must not fall back to the body size"
    );
    assert_eq!(style.weight, FontWeight::Bold);
    let block = layout.pages[0]
        .blocks
        .iter()
        .find(|block| block.node == node)
        .expect("caption block");
    assert!(block_text(&layout, block).contains("note"));
}

#[test]
fn a_document_without_caption_rules_still_renders_captions() {
    let source = r#"
name: plain
page:
  size: custom
  width: 160mm
  height: 220mm
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
"#;
    let doc = document(vec![figure(vec![text("架构")])]);
    let styled = resolve(&doc, &parse_template(source).expect("template")).expect("styles");
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let figure = caption(&layout, "架构");

    assert!(
        !figure.lines.is_empty(),
        "the caption inherits the document style"
    );
}
