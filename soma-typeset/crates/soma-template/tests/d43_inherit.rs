//! D43: pure-data template inheritance. The host reads the two YAML sources;
//! the crate merges values and validates the result without filesystem access.

use soma_core::Width;
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_template::{parse_template, parse_template_inherit, PageSize};

const BASE: &str = r#"
name: base
page:
  size: A5
  margin: 12mm
  margin-top: 18mm
toc:
  depth: 2
  title: 目录
table:
  header-rule-width: 0.6pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
"#;

fn document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), "正文").into()],
    )));
    document.renumber();
    document
}

#[test]
fn an_overlay_inherits_sections_and_extends_styles() {
    let overlay = r#"
version: "2"
page:
  margin-left: 8mm
styles:
  - selector:
      element: heading
    declarations:
      weight: bold
"#;
    let base = parse_template(BASE).expect("base");
    let merged = parse_template_inherit(BASE, overlay).expect("merged");

    assert_eq!(merged.name, "base");
    assert_eq!(merged.version.as_deref(), Some("2"));
    assert_eq!(merged.page.size, PageSize::A5, "page size inherited");
    assert_eq!(merged.page.margin_top, base.page.margin_top);
    assert_eq!(
        merged.page.margin_left,
        soma_template::PageLength::Length(soma_template::LengthSpec::parse("8mm").expect("8mm")),
        "overlay overrides one page key"
    );
    assert!(merged.toc.is_some(), "toc inherited");
    assert_eq!(merged.toc.as_ref().map(|toc| toc.depth), Some(2));
    assert_eq!(merged.table.header_rule, base.table.header_rule);
    assert_eq!(merged.styles.len(), base.styles.len() + 1);
    assert_eq!(
        merged.styles.last().unwrap().selector.element.as_deref(),
        Some("heading")
    );
    assert_ne!(merged.template_hash(), base.template_hash());
}

#[test]
fn an_overlay_rule_wins_the_cascade_on_equal_specificity() {
    let overlay = r#"
styles:
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
"#;
    let document = document();
    let merged = parse_template_inherit(BASE, overlay).expect("merged");
    let base = parse_template(BASE).expect("base");

    let base_style = soma_template::resolve(&document, &base).expect("base styles");
    let base_id = document.blocks[0].meta().id;
    assert_eq!(
        base_style
            .style(base_style.style_of(base_id).unwrap())
            .unwrap()
            .first_indent,
        Width(8192)
    );

    let merged_style = soma_template::resolve(&document, &merged).expect("merged styles");
    let merged_id = document.blocks[0].meta().id;
    assert_eq!(
        merged_style
            .style(merged_style.style_of(merged_id).unwrap())
            .unwrap()
            .first_indent,
        Width::ZERO
    );
}

#[test]
fn an_explicit_null_disables_an_inherited_toc() {
    let merged = parse_template_inherit(BASE, "toc:\n").expect("merged");
    assert!(merged.toc.is_none());
    assert_eq!(merged.name, "base");
}

#[test]
fn overlay_diagnostics_point_at_the_overlay_source() {
    let overlay = "\nunknown-section: 1\n";
    let error = parse_template_inherit(BASE, overlay).expect_err("unknown key");
    assert_eq!(error.reason, "unknown key");
    assert_eq!(error.line, 2);
    assert_eq!(error.column, 1);
}

#[test]
fn merged_masters_are_validated_together() {
    let base = r#"
name: masters
page:
  size: A5
masters:
  - name: plain
    header: 0
    footer: 0
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let overlay = r#"
masters:
  - name: plain
    header: 1mm
    footer: 1mm
"#;
    let error = parse_template_inherit(base, overlay).expect_err("duplicate master");
    assert_eq!(error.reason, "duplicate or reserved master name");
}

#[test]
fn an_incomplete_base_is_rejected_before_merging() {
    let base = r#"
name: no-page
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let error = parse_template_inherit(base, "version: \"1\"\n").expect_err("no page");
    assert_eq!(error.reason, "missing required key");
}
