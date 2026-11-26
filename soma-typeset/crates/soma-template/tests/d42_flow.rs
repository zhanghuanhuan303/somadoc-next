//! D42: one-line flow sequences and flow mappings in the YAML subset.

use soma_template::{parse_template, Property, PropertyValue};

#[test]
fn flow_collections_fill_selectors_and_declarations() {
    let source = r#"
name: flow
page: {size: A4, margin: 20mm}
styles:
  - selector: {element: document}
    declarations: {font-size: 10.5pt, line-height: 1.6}
  - selector: {element: paragraph, classes: [first, "wide,]"] , level: 2}
    declarations: {first-indent: 2em, space-after: 0}
"#;
    let template = parse_template(source).expect("flow template must parse");

    assert_eq!(template.page.size, soma_template::PageSize::A4);
    assert_eq!(template.styles.len(), 2);
    assert_eq!(
        template.styles[1].selector.classes,
        vec![String::from("first"), String::from("wide,]")]
    );
    assert_eq!(template.styles[1].selector.level, Some(2));

    let first_indent = template.styles[1]
        .declarations
        .iter()
        .find(|declaration| declaration.property == Property::FirstIndent)
        .expect("first-indent");
    assert!(matches!(first_indent.value, PropertyValue::Length(_)));

    let json = template.canonical_json();
    assert!(
        json.contains("\"classes\":[\"first\",\"wide,]\"]"),
        "{json}"
    );
    assert_eq!(json, template.canonical_json(), "deterministic");
}

#[test]
fn a_whole_style_sequence_can_be_a_flow_collection() {
    let source = r#"
name: flow-styles
page: {size: A5, margin: 10mm}
styles: [{selector: {element: document}, declarations: {font-size: 10pt}}]
"#;
    let template = parse_template(source).expect("flow styles must parse");
    assert_eq!(template.styles.len(), 1);
    assert_eq!(
        template.styles[0].selector.element.as_deref(),
        Some("document")
    );

    // A block sequence is still accepted and produces the same declarations.
    let block = r#"
name: flow-styles
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
"#;
    assert_eq!(
        template.canonical_json(),
        parse_template(block).unwrap().canonical_json()
    );
}

#[test]
fn malformed_flow_collections_report_a_position() {
    let cases = [
        (
            "\nname: bad\nstyles:\n  - selector: {element: document}\n    declarations: {font-size: 10pt}\n  - selector: {element: paragraph, classes: [a, b}\n    declarations: {first-indent: 0}\n",
            "expected `,` or `]` in flow sequence",
        ),
        (
            "\nname: bad\nstyles: [{selector: {element: document}, declarations: {font-size: 10pt}]\n",
            "expected `,` or `}` in flow mapping",
        ),
        (
            "\nname: bad\nstyles:\n  - selector: {element: document}\n    declarations: {font-size: 10pt} junk\n",
            "unexpected text after flow collection",
        ),
        (
            "\nname: bad\nstyles:\n  - selector: {element: document, element: paragraph}\n    declarations: {font-size: 10pt}\n",
            "duplicate key",
        ),
    ];

    for (source, reason) in cases {
        let error = parse_template(source).expect_err("malformed flow must fail");
        assert_eq!(error.reason, reason, "{error}");
        assert!(error.line > 1, "{error}");
        assert!(error.column > 0, "{error}");
    }
}

#[test]
fn unknown_keys_inside_flow_mappings_keep_their_position() {
    let source = "\nname: bad\npage: {size: A5}\nstyles:\n  - selector: {element: document}\n    declarations: {font-size: 10pt}\n  - selector: {element: paragraph, bogus: 1}\n    declarations: {first-indent: 0}\n";
    let error = parse_template(source).expect_err("unknown key must fail");
    assert_eq!(error.reason, "unknown key");
    assert_eq!(error.line, 7);
    assert!(error.column > 1, "{error}");
}
