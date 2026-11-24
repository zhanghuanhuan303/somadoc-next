//! D44: typed percentages, resolved only where a base is defined.

use soma_core::Width;
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_template::{
    parse_template, resolve, LengthSpec, PageLength, Percent, Property, PropertyValue, Rational,
};

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
fn percent_parsing_is_exact_and_strict() {
    let half = Percent::parse("50%").expect("50%");
    assert_eq!(half.value(), Rational::new(1, 2).unwrap());

    let one_and_a_half = Percent::parse("1.5%").expect("1.5%");
    assert_eq!(one_and_a_half.value(), Rational::new(3, 200).unwrap());

    assert!(Percent::parse("-2.5%").is_some());
    assert!(Percent::parse("50 %").is_none(), "space before %");
    assert!(Percent::parse("50percent").is_none());
    assert!(Percent::parse("%").is_none());
    assert!(Percent::parse("").is_none());

    // The existing unit parser is untouched: a percentage is not an em length.
    assert!(LengthSpec::parse("50%").is_none());
    assert!(LengthSpec::parse("50").is_none());

    // Percent::of needs an explicit base and truncates toward zero.
    assert_eq!(Percent::parse("50%").unwrap().of(Width(1001)), Width(500));
    assert_eq!(Percent::parse("33%").unwrap().of(Width(1000)), Width(330));
    assert_eq!(Percent::parse("-33%").unwrap().of(Width(1000)), Width(-330));
}

#[test]
fn page_margins_resolve_against_the_page_width() {
    let source = r#"
name: percent
page:
  size: custom
  width: 100mm
  height: 100mm
  margin: 10%
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let template = parse_template(source).expect("percentage margins parse");
    assert!(template.page.margin_top.is_percent());
    assert_eq!(
        template.page.margin_top,
        PageLength::Percent(Percent::parse("10%").unwrap())
    );

    let styled = resolve(&document(), &template).expect("percentage margins resolve");
    let page = styled.page();
    assert_eq!(page.margin_top, Width(page.width.0 / 10));
    assert_eq!(page.margin_bottom, Width(page.width.0 / 10));
    assert_eq!(page.margin_left, Width(page.width.0 / 10));
    assert_eq!(page.margin_right, Width(page.width.0 / 10));
    assert_eq!(
        page.content_width(),
        Width(page.width.0 - 2 * page.margin_left.0)
    );
}

#[test]
fn individual_margins_can_mix_percentages_and_lengths() {
    let source = r#"
name: mixed-margins
page:
  size: A5
  margin: 12mm
  margin-top: 25%
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("mixed margins parse");
    let styled = resolve(&document(), &template).expect("mixed margins resolve");
    let page = styled.page();
    assert_eq!(page.margin_top, Width(page.width.0 / 4));
    assert_ne!(page.margin_top, page.margin_left);
}

#[test]
fn line_height_percent_uses_the_resolved_font_size() {
    let source = r#"
name: line-height-percent
page:
  size: A5
  margin: 12mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 120%
"#;
    let document = document();
    let template = parse_template(source).expect("percentage line-height parses");
    let styled = resolve(&document, &template).expect("resolves");

    let style = styled
        .style(styled.style_of(document.blocks[0].meta().id).unwrap())
        .unwrap();
    assert_eq!(style.font_size, Width::EM);
    assert_eq!(style.line_height, Width(4915), "6/5 of 4096 truncates");
}

#[test]
fn a_percentage_without_a_base_is_rejected_at_resolve() {
    let source = r#"
name: unsupported
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
      first-indent: 50%
"#;
    let template = parse_template(source).expect("a typed percentage still parses");
    let rule = template
        .styles
        .iter()
        .find(|rule| rule.selector.element.as_deref() == Some("paragraph"))
        .expect("paragraph rule");
    assert!(matches!(
        rule.declarations[0].value,
        PropertyValue::Percent(_)
    ));
    assert_eq!(rule.declarations[0].property, Property::FirstIndent);

    let error = resolve(&document(), &template).expect_err("no width base here");
    assert_eq!(
        error.reason, "percentage is not supported for this property",
        "{error}"
    );
    assert!(error.line > 1, "{error}");
}

#[test]
fn percentage_contexts_without_a_defined_base_fail_at_parse_time() {
    let document_font = r#"
name: bad-font
page:
  size: A5
styles:
  - selector:
      element: document
    declarations:
      font-size: 50%
"#;
    let error = parse_template(document_font).expect_err("font-size percent");
    assert_eq!(
        error.reason, "document font-size must use an absolute unit",
        "{error}"
    );

    let page_header = r#"
name: bad-header
page:
  size: A5
  header: 10%
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let error = parse_template(page_header).expect_err("header percent");
    assert_eq!(error.reason, "invalid length value", "{error}");

    let page_width = r#"
name: bad-width
page:
  size: custom
  width: 50%
  height: 100mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let error = parse_template(page_width).expect_err("page width percent");
    assert_eq!(error.reason, "invalid length value", "{error}");

    let table_rule = r#"
name: bad-rule
page:
  size: A5
table:
  rule-width: 10%
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let error = parse_template(table_rule).expect_err("rule percent");
    assert_eq!(error.reason, "invalid length value", "{error}");
}

#[test]
fn canonical_output_marks_percentages_without_changing_lengths() {
    let percent = r#"
name: percent
page:
  size: A5
  margin: 10%
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let json = parse_template(percent).unwrap().canonical_json();
    assert!(json.contains("\"unit\":\"%\""), "{json}");

    let length = r#"
name: percent
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    let json = parse_template(length).unwrap().canonical_json();
    assert!(!json.contains("\"unit\":\"%\""), "{json}");
    assert!(json.contains("\"unit\":\"mm\""), "{json}");
}
