//! D47: selector combinators — immediate parent and ordered descendant chains.

use soma_core::Width;
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};
use soma_template::{parse_template, resolve, Selector};

fn paragraph(value: &str) -> Block {
    Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), value).into()],
    ))
}

fn cell(value: &str) -> Cell {
    Cell::new(NodeMeta::synthesized(), vec![paragraph(value)])
}

/// `body` paragraph, a table with one cell-paragraph, and a quote paragraph.
fn document() -> Document {
    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)],
        vec![Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("单元文字")],
        )],
    );
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(paragraph("正文"));
    document.push_block(Block::Table(table));
    document.push_block(Block::BlockQuote(soma_doc::block::BlockQuote::new(
        NodeMeta::synthesized(),
        vec![paragraph("引文")],
    )));
    document.renumber();
    document
}

#[test]
fn parent_matches_only_the_immediate_parent() {
    let selector = Selector {
        element: Some(String::from("paragraph")),
        parent: Some(String::from("cell")),
        ..Selector::default()
    };

    assert!(selector.matches("paragraph", None, &[], &["table", "row", "cell"], None));
    assert!(!selector.matches("paragraph", None, &[], &["table", "row"], None));
    assert!(!selector.matches("paragraph", None, &[], &[], None));

    let selector = Selector {
        element: Some(String::from("paragraph")),
        parent: Some(String::from("blockQuote")),
        ..Selector::default()
    };
    assert!(selector.matches("paragraph", None, &[], &["blockQuote"], None));
    assert!(
        !selector.matches("paragraph", None, &[], &["table", "row", "cell"], None),
        "a cell is not a blockQuote even if a quote is further up"
    );
    assert!(!selector.matches("paragraph", None, &[], &["blockQuote", "listItem"], None));
}

#[test]
fn ancestors_match_an_ordered_descendant_chain() {
    let selector = Selector {
        element: Some(String::from("paragraph")),
        ancestors: vec![String::from("table"), String::from("cell")],
        ..Selector::default()
    };

    assert!(selector.matches("paragraph", None, &[], &["table", "row", "cell"], None));
    assert!(
        !selector.matches("paragraph", None, &[], &["row", "cell"], None),
        "both ancestors are required"
    );
    assert!(
        !selector.matches("paragraph", None, &[], &["cell", "table"], None),
        "the chain is ordered outermost first"
    );
    assert!(
        !selector.matches("paragraph", None, &[], &["table", "row"], None),
        "the chain must reach the innermost name"
    );

    let no_match = Selector {
        element: Some(String::from("paragraph")),
        ancestors: vec![String::from("table"), String::from("blockQuote")],
        ..Selector::default()
    };
    assert!(!no_match.matches("paragraph", None, &[], &["table", "row", "cell"], None));
}

#[test]
fn combinator_rules_survive_the_template_round_trip() {
    let source = r#"
name: combinators
page:
  size: A5
  margin: 12mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector: {element: paragraph, parent: cell}
    declarations:
      first-indent: 0.5em
  - selector:
      element: paragraph
      ancestors: [table, cell]
    declarations:
      indent-left: 0.25em
  - selector: {element: paragraph, parent: blockQuote}
    declarations:
      first-indent: 3em
"#;
    let template = parse_template(source).expect("combinator selectors parse");
    assert_eq!(template.styles[2].selector.parent.as_deref(), Some("cell"));
    assert_eq!(
        template.styles[3].selector.ancestors,
        vec![String::from("table"), String::from("cell")]
    );

    let json = template.canonical_json();
    assert!(json.contains("\"parent\":\"cell\""), "{json}");
    assert!(
        json.contains("\"ancestors\":[\"table\",\"cell\"]"),
        "{json}"
    );

    let document = document();
    let body_id = document.blocks[0].meta().id;
    let (cell_id, quote_id) = match &document.blocks[1..] {
        [Block::Table(table), Block::BlockQuote(quote)] => (
            table.rows[0].cells[0].blocks[0].meta().id,
            quote.blocks[0].meta().id,
        ),
        other => panic!("unexpected document shape: {other:?}"),
    };

    let styled = resolve(&document, &template).expect("resolve");
    let style = |id| {
        styled
            .style(styled.style_of(id).expect("node style"))
            .unwrap()
    };

    assert_eq!(style(body_id).first_indent, Width(8192), "body stays 2em");
    assert_eq!(
        style(cell_id).first_indent,
        Width(2048),
        "0.5em inside the cell (the later parent rule wins)"
    );
    assert_eq!(
        style(cell_id).indent_left,
        Width(1024),
        "the descendant chain adds 0.25em of the cell font"
    );
    assert_eq!(
        style(quote_id).first_indent,
        Width(12288),
        "3em inside the quote"
    );
}

#[test]
fn unknown_combinator_names_are_rejected() {
    let source = r#"
name: bad-combinator
page:
  size: A5
styles:
  - selector: {element: document}
    declarations: {font-size: 10pt}
  - selector: {element: paragraph, parent: cel}
    declarations: {first-indent: 0}
"#;
    let error = parse_template(source).expect_err("unknown parent");
    assert_eq!(error.reason, "unknown parent selector", "{error}");

    let source = r#"
name: bad-ancestor
page:
  size: A5
styles:
  - selector: {element: document}
    declarations: {font-size: 10pt}
  - selector: {element: paragraph, ancestors: [cell, cel]}
    declarations: {first-indent: 0}
"#;
    let error = parse_template(source).expect_err("unknown ancestor");
    assert_eq!(error.reason, "unknown ancestor selector", "{error}");
}
