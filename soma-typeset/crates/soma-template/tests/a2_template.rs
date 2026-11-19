//! A2 template parsing: YAML subset behaviour, validation, hashing.

use soma_template::{parse_template, FootnoteNumbering, PageSize, Property, Unit};

const FULL: &str = r##"
# a book template
name: book
version: "1"
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

#[test]
fn parses_a_full_template() {
    let template = parse_template(FULL).expect("fixture must parse");

    assert_eq!(template.name, "book");
    assert_eq!(template.version.as_deref(), Some("1"));
    assert_eq!(template.page.size, PageSize::A4);
    assert_eq!(template.footnote.numbering, FootnoteNumbering::Page);
    assert_eq!(template.styles.len(), 4);

    // 10.5pt becomes the exact rational 21/2.
    assert_eq!(template.body_font_pt().num(), 21);
    assert_eq!(template.body_font_pt().den(), 2);

    let paragraph = &template.styles[1];
    assert_eq!(paragraph.selector.element.as_deref(), Some("paragraph"));
    match &paragraph.declarations[0].value {
        soma_template::PropertyValue::Length(length) => {
            assert_eq!(length.unit, Unit::Em);
            assert_eq!(length.value.num(), 2);
            assert_eq!(length.value.den(), 1);
        }
        other => panic!("expected a length, got {other:?}"),
    }

    let heading = &template.styles[3];
    assert_eq!(heading.selector.classes, vec!["chapter".to_string()]);
    assert!(heading
        .declarations
        .iter()
        .any(|declaration| declaration.property == Property::KeepWithNext));
}

#[test]
fn canonical_json_is_stable_and_namespaced() {
    let template = parse_template(FULL).expect("fixture must parse");
    let json = template.canonical_json();

    assert!(
        json.starts_with("{\"schema\":\"soma.template/3\""),
        "{json}"
    );
    assert!(json.contains("\"size\":\"A4\""));
    assert!(json.contains("\"unit\":\"pt\""));
    assert!(json.contains("\"numbering\":\"page\""));
    assert_eq!(json, template.canonical_json());
    assert_eq!(json, parse_template(FULL).unwrap().canonical_json());
}

#[test]
fn template_hash_is_sensitive_to_the_declarations() {
    let first = parse_template(FULL).expect("fixture must parse");
    let second = parse_template(&FULL.replace("2em", "2.5em")).expect("edited fixture must parse");

    assert_eq!(
        first.template_hash(),
        parse_template(FULL).unwrap().template_hash()
    );
    assert_ne!(first.template_hash(), second.template_hash());
}

#[test]
fn unknown_property_is_rejected_with_a_position() {
    let source = FULL.replace("first-indent: 2em", "first-indnet: 2em");
    let error = parse_template(&source).expect_err("typo must fail");

    assert_eq!(error.reason, "unknown style property");
    assert!(
        error.line > 1,
        "line must point at the declaration: {error}"
    );
    assert!(error.path.starts_with("styles[1]"), "path: {error}");
}

#[test]
fn unknown_key_and_element_are_rejected() {
    let error = parse_template(&FULL.replace("version: \"1\"", "verison: \"1\""))
        .expect_err("typo must fail");
    assert_eq!(error.reason, "unknown key");

    let error = parse_template(&FULL.replace("element: paragraph", "element: paragraf"))
        .expect_err("typo must fail");
    assert_eq!(error.reason, "unknown element selector");
}

#[test]
fn unknown_page_size_is_rejected() {
    let error = parse_template(&FULL.replace("size: A4", "size: B5")).expect_err("size must fail");
    assert_eq!(error.reason, "page size must be A4, A5, Letter, or custom");
}

#[test]
fn duplicate_keys_are_rejected() {
    let source = FULL.replace("name: book", "name: book\nname: other");
    let error = parse_template(&source).expect_err("duplicate must fail");
    assert_eq!(error.reason, "duplicate key");
}

#[test]
fn the_document_font_size_is_required_and_absolute() {
    let missing = FULL.replace("      font-size: 10.5pt\n", "");
    let error = parse_template(&missing).expect_err("font-size is required");
    assert_eq!(
        error.reason,
        "template must set font-size on the document element"
    );

    let relative = FULL.replace("font-size: 10.5pt", "font-size: 1.2em");
    let error = parse_template(&relative).expect_err("relative root font is invalid");
    assert_eq!(error.reason, "document font-size must use an absolute unit");
}

#[test]
fn invalid_values_are_rejected() {
    let error =
        parse_template(&FULL.replace("line-height: 1.6", "line-height: tall")).expect_err("bad");
    assert_eq!(error.reason, "invalid line-height value");

    let error = parse_template(&FULL.replace("first-indent: 2em", "first-indent: 2"))
        .expect_err("bare non-zero number is ambiguous");
    assert_eq!(error.reason, "invalid length value");

    let error = parse_template(&FULL.replace("color: 101010", "color: red")).expect_err("bad");
    assert_eq!(error.reason, "color must be #rrggbb");

    let error = parse_template(&FULL.replace("keep-with-next: true", "keep-with-next: maybe"))
        .expect_err("bad");
    assert_eq!(error.reason, "expected true or false");
}

#[test]
fn comments_and_quoted_scalars_are_handled() {
    let source = r#"
name: 'quo''ted'   # trailing comment
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      family: "Noto Serif CJK SC"
"#;
    let template = parse_template(source).expect("fixture must parse");
    assert_eq!(template.name, "quo'ted");
    assert!(template
        .styles
        .iter()
        .flat_map(|rule| &rule.declarations)
        .any(|declaration| matches!(
            &declaration.value,
            soma_template::PropertyValue::Text(text) if text == "Noto Serif CJK SC"
        )));
}

#[test]
fn a_within_selector_parses_and_canonicalizes() {
    let source = r#"
name: cells
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: paragraph
      within: cell
    declarations:
      first-indent: 0
      indent-left: 0.25em
"#;
    let template = parse_template(source).expect("within must parse");
    let rule = template
        .styles
        .iter()
        .find(|rule| rule.selector.within.as_deref() == Some("cell"))
        .expect("cell-context rule");
    assert_eq!(rule.selector.element.as_deref(), Some("paragraph"));

    let json = template.canonical_json();
    assert!(json.contains("\"within\":\"cell\""), "{json}");
    assert_eq!(json, template.canonical_json(), "deterministic");

    let error = parse_template(&source.replace("within: cell", "within: celll")).expect_err("bad");
    assert_eq!(error.reason, "unknown within selector");
    assert!(
        !rule.selector.matches("paragraph", None, &[], &[]),
        "within: cell must not match outside a cell"
    );
    assert!(
        rule.selector
            .matches("paragraph", None, &[], &["table", "row", "cell"]),
        "within: cell matches inside a cell"
    );
}

#[test]
fn a_table_block_parses_and_canonicalizes() {
    let source = r#"
name: ruled
page:
  size: A4
table:
  rule-width: 0.5pt
  header-rule-width: 0.75pt
  outer-rule-width: 1pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("table section must parse");
    assert_eq!(
        template.table.rule,
        soma_template::LengthSpec::parse("0.5pt").unwrap()
    );
    assert_eq!(
        template.table.header_rule,
        soma_template::LengthSpec::parse("0.75pt").unwrap()
    );
    assert_eq!(
        template.table.outer_rule,
        soma_template::LengthSpec::parse("1pt").unwrap()
    );
    assert!(!template.table.is_zero());

    let json = template.canonical_json();
    assert!(
        json.contains("\"table\":{\"ruleWidth\":{\"num\":1,\"den\":2,\"unit\":\"pt\"}"),
        "{json}"
    );

    let error = parse_template(&source.replace("rule-width: 0.5pt", "rule-width: -0.5pt"))
        .expect_err("negative rules are invalid");
    assert_eq!(error.reason, "table rule width must not be negative");

    let error = parse_template(&source.replace("outer-rule-width: 1pt", "outer: 1pt"))
        .expect_err("unknown table key");
    assert!(error.reason.contains("unknown"));
}
