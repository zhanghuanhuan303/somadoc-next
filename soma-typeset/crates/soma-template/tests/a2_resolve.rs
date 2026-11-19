//! A2 resolve: cascade, inheritance, page geometry, style hashing.

use soma_core::Width;
use soma_doc::block::{Heading, Paragraph};
use soma_doc::inline::Container;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Attrs, Block, Document, Inline, Text};
use soma_template::{
    parse_template, resolve, FontWeight, FootnoteNumbering, PropertyValue, Template,
};

const BOOK: &str = r##"
name: book
page:
  size: A4
  margin: 20mm
footnote:
  numbering: page
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
      family: serif
      color: 101010
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
      space-after: 0
  - selector:
      element: strong
    declarations:
      weight: bold
  - selector:
      element: heading
      class: chapter
    declarations:
      font-size: 20pt
      space-before: 1.5em
      keep-with-next: true
"##;

fn paragraph(attrs: Attrs, inlines: Vec<Inline>) -> Block {
    Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized().with_attrs(attrs),
        inlines,
    ))
}

fn document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Heading(Heading::new(
        NodeMeta::synthesized().with_attrs(Attrs::new().with_class("chapter")),
        1,
        vec![Text::new(NodeMeta::synthesized(), "第一章").into()],
    )));
    document.push_block(paragraph(
        Attrs::new(),
        vec![
            Text::new(NodeMeta::synthesized(), "正文").into(),
            Inline::Strong(Container::new(
                NodeMeta::synthesized(),
                vec![Text::new(NodeMeta::synthesized(), "强调").into()],
            )),
        ],
    ));
    document.renumber();
    document
}

fn book() -> Template {
    parse_template(BOOK).expect("fixture must parse")
}

#[test]
fn node_styles_are_sorted_and_complete() {
    let document = document();
    let styled = resolve(&document, &book()).expect("resolve");

    let ids: Vec<u32> = styled.node_style().iter().map(|(id, _)| id.0).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len() as u32, document.node_count());

    for (node, style) in styled.node_style() {
        assert!(
            styled.style(*style).is_some(),
            "node {} points at a missing style",
            node.0
        );
    }
}

#[test]
fn paragraph_inherits_font_and_gets_its_indent() {
    let document = document();
    let styled = resolve(&document, &book()).expect("resolve");

    let paragraph_node = document.blocks[1].meta().id;
    let style = styled
        .style(styled.style_of(paragraph_node).expect("paragraph style"))
        .expect("style exists");

    assert_eq!(style.font_size, Width::EM);
    assert_eq!(style.first_indent, Width(8192));
    assert_eq!(style.line_height, Width(6553));
    assert_eq!(style.color, Some(0x101010));
    assert_eq!(style.family.as_deref(), Some("serif"));
}

#[test]
fn inline_styles_inherit_and_override() {
    let document = document();
    let styled = resolve(&document, &book()).expect("resolve");

    let strong = match &document.blocks[1] {
        Block::Paragraph(paragraph) => paragraph.inlines[1].meta().id,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    let style = styled
        .style(styled.style_of(strong).expect("strong style"))
        .expect("style exists");

    assert_eq!(style.weight, FontWeight::Bold);
    // Inherited from the paragraph.
    assert_eq!(style.font_size, Width::EM);
    assert_eq!(style.color, Some(0x101010));
    // Not inherited.
    assert_eq!(style.first_indent, Width::ZERO);
}

#[test]
fn heading_uses_absolute_font_size_and_em_spacing() {
    let document = document();
    let styled = resolve(&document, &book()).expect("resolve");

    let heading_node = document.blocks[0].meta().id;
    let style = styled
        .style(styled.style_of(heading_node).expect("heading style"))
        .expect("style exists");

    // 20pt at a 10.5pt body: 20/10.5 * 4096 = 7801.9..., truncated.
    assert_eq!(style.font_size, Width(7801));
    // 1.5em of the heading's own font size.
    assert_eq!(style.space_before, Width(11701));
    assert!(style.keep_with_next);
}

#[test]
fn page_geometry_is_resolved_in_em_units() {
    let styled = resolve(&document(), &book()).expect("resolve");
    let page = styled.page();

    assert_eq!(page.width, Width(232214));
    assert_eq!(page.margin_left, Width(22115));
    assert_eq!(page.content_width(), Width(187984));
    assert_eq!(
        page.content_height(),
        Width(page.height.0 - 2 * page.margin_top.0)
    );
    assert_eq!(styled.footnote_numbering(), FootnoteNumbering::Page);
}

#[test]
fn class_selectors_beat_element_selectors() {
    let source = r##"
name: specific
page:
  size: A5
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      color: 111111
  - selector:
      element: paragraph
    declarations:
      color: 222222
  - selector:
      element: paragraph
      class: lead
    declarations:
      color: 333333
"##;
    let template = parse_template(source).expect("template");

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(paragraph(
        Attrs::new(),
        vec![Text::new(NodeMeta::synthesized(), "普通").into()],
    ));
    document.push_block(paragraph(
        Attrs::new().with_class("lead"),
        vec![Text::new(NodeMeta::synthesized(), "导语").into()],
    ));
    document.renumber();

    let styled = resolve(&document, &template).expect("resolve");
    let plain = styled
        .style_of(document.blocks[0].meta().id)
        .and_then(|id| styled.style(id))
        .expect("plain style");
    let lead = styled
        .style_of(document.blocks[1].meta().id)
        .and_then(|id| styled.style(id))
        .expect("lead style");

    assert_eq!(plain.color, Some(0x222222));
    assert_eq!(lead.color, Some(0x333333));
}

#[test]
fn later_rules_win_on_equal_specificity() {
    let source = r##"
name: ties
page:
  size: A5
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: paragraph
    declarations:
      color: 111111
  - selector:
      element: paragraph
    declarations:
      color: 999999
"##;
    let template = parse_template(source).expect("template");
    let styled = resolve(&document(), &template).expect("resolve");
    let style = styled
        .style_of(document().blocks[1].meta().id)
        .and_then(|id| styled.style(id))
        .expect("style");

    assert_eq!(style.color, Some(0x999999));
}

#[test]
fn every_alignment_resolves() {
    for (value, expected) in [
        ("justify", soma_core::Align::Justify),
        ("left", soma_core::Align::Left),
        ("center", soma_core::Align::Center),
        ("right", soma_core::Align::Right),
    ] {
        let source = format!(
            r##"
name: align
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      align: {value}
"##
        );
        let template = parse_template(&source).expect("template parses");
        let styled = resolve(&document(), &template).expect("all four alignments resolve");
        let root = styled
            .table()
            .get(soma_core::StyleId(0))
            .expect("root style");
        assert_eq!(root.align, expected, "{value}");
    }

    // A value outside the set is still a parse error.
    let bad = "name: n\npage:\n  size: A4\nstyles:\n  - selector:\n      element: document\n    declarations:\n      font-size: 10.5pt\n      align: sideways\n";
    let error = parse_template(bad).expect_err("unknown alignment");
    assert!(error.reason.contains("align must be"), "{}", error.reason);
}

#[test]
fn style_hash_tracks_the_resolution() {
    let doc = document();
    let first = resolve(&doc, &book()).expect("resolve");
    let second = resolve(&document(), &book()).expect("resolve");
    assert_eq!(first.style_hash(), second.style_hash());
    assert_eq!(first.canonical_json(), second.canonical_json());

    let other = resolve(
        &doc,
        &parse_template(&BOOK.replace("first-indent: 2em", "first-indent: 3em")).expect("template"),
    )
    .expect("resolve");
    assert_ne!(first.style_hash(), other.style_hash());

    // Changing only the document content does not change the styles.
    let mut edited = doc.clone();
    if let Block::Paragraph(paragraph) = &mut edited.blocks[1] {
        paragraph.inlines[0] = Text::new(NodeMeta::synthesized(), "改").into();
    }
    edited.renumber();
    let edited_styled = resolve(&edited, &book()).expect("resolve");
    assert_eq!(first.style_hash(), edited_styled.style_hash());
}

#[test]
fn computed_styles_are_deduplicated() {
    let document = document();
    let styled = resolve(&document, &book()).expect("resolve");

    for (index, style) in styled.table().styles().iter().enumerate() {
        for other in styled.table().styles()[..index].iter() {
            assert_ne!(style, other, "duplicate computed style in the table");
        }
    }
}

#[test]
fn declarations_are_typed() {
    let template = book();
    let document_rule = &template.styles[0];
    assert!(document_rule
        .declarations
        .iter()
        .any(|declaration| matches!(declaration.value, PropertyValue::Color(0x101010))));
    assert!(document_rule
        .declarations
        .iter()
        .any(|declaration| matches!(declaration.value, PropertyValue::Text(_))));
}
