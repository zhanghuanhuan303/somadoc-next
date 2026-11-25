//! `soma.engine/1` parsing, validation, canonical form, and layout wiring.

use soma_core::{BreakConfig, ParamSet, VerticalParams};
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_engine::{EngineConfig, SCHEMA};
use soma_layout::{layout_with, PaginationStrategy, StyledMetrics};
use soma_metrics::MetricsTable;
use soma_template::{parse_template, resolve};

const DEFAULTS: &str = r#"
schema: soma.engine/1
preset: publish-v1
window: paragraph
max_kp_atoms: null
fallback:
  allow_emergency: true
  allow_forbidden: true
  allow_overfull: true
  emergency_stretch: 3em
vertical:
  page_weight: 0
  badness_scale: 100
  overflow_penalty: 1000000
  club_penalty: 10000
  widow_penalty: 10000
"#;

fn document() -> Document {
    let mut doc = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    for index in 0..8 {
        let text = if index % 3 == 0 {
            "中文排版需要断行与对齐。".repeat(30)
        } else {
            String::from("段落 English 与数字 123，用来测试分页。")
        };
        doc.push_block(Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), text).into()],
        )));
    }
    doc.renumber();
    doc
}

const TEMPLATE: &str = r#"
name: engine-fixture
page:
  size: A5
  margin: 15mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
"#;

#[test]
fn documented_defaults_match_the_built_in_preset() {
    let config = EngineConfig::parse(DEFAULTS).expect("parse");
    let built_in = BreakConfig::default();

    assert_eq!(config.preset(), "publish-v1");
    assert_eq!(
        config.break_config().param_hash(),
        BreakConfig::from_params(ParamSet::publish_v1()).param_hash()
    );
    let _ = built_in;
    assert_eq!(config.vertical(), VerticalParams::default_v1());
    assert_eq!(config.hash(), EngineConfig::parse(DEFAULTS).unwrap().hash());
    assert!(config.canonical_json().contains(SCHEMA));
}

#[test]
fn a_minimal_document_is_enough() {
    let config = EngineConfig::parse("schema: soma.engine/1\n").expect("parse");
    assert_eq!(
        config.break_config().param_hash(),
        BreakConfig::from_params(ParamSet::publish_v1()).param_hash()
    );
    assert_eq!(config.vertical(), VerticalParams::default_v1());
}

#[test]
fn comments_and_quoted_values_are_handled() {
    let source = r#"
# engine configuration
schema: "soma.engine/1"   # namespaced
preset: 'default-v1'
window: "paragraph"
"#;
    let config = EngineConfig::parse(source).expect("parse");
    assert_eq!(config.preset(), "default-v1");
}

#[test]
fn overrides_change_the_relevant_hash() {
    let source = DEFAULTS.replace("widow_penalty: 10000", "widow_penalty: 20000");
    let config = EngineConfig::parse(&source).expect("parse");
    assert_ne!(
        config.vertical().hash(),
        VerticalParams::default_v1().hash()
    );
    assert!(config.vertical().widow_penalty > 10_000);
    assert_eq!(
        config.break_config().param_hash(),
        BreakConfig::from_params(ParamSet::publish_v1()).param_hash(),
        "vertical knobs must not change the inline parameter hash"
    );

    let source = DEFAULTS.replace("allow_forbidden: true", "allow_forbidden: false");
    let config = EngineConfig::parse(&source).expect("parse");
    assert_ne!(
        config.break_config().param_hash(),
        BreakConfig::from_params(ParamSet::publish_v1()).param_hash()
    );

    let source = DEFAULTS.replace("max_kp_atoms: null", "max_kp_atoms: 512");
    let config = EngineConfig::parse(&source).expect("parse");
    assert_eq!(
        config.break_config().params().break_params.max_kp_atoms,
        Some(512)
    );
}

#[test]
fn the_canonical_hash_changes_with_content() {
    let first = EngineConfig::parse(DEFAULTS).expect("parse");
    let second = EngineConfig::parse(&DEFAULTS.replace("club_penalty: 10000", "club_penalty: 1"))
        .expect("parse");
    assert_ne!(first.hash(), second.hash());
    assert_ne!(first.canonical_json(), second.canonical_json());
}

#[test]
fn unknown_keys_and_sections_are_rejected() {
    let cases = [
        "schema: soma.engine/1\nmystery: 1\n",
        "schema: soma.engine/1\nfallback:\n  mystery: true\n",
        "schema: soma.engine/1\nvertical:\n  mystery: 1\n",
        "schema: soma.engine/1\nmystery:\n  a: 1\n",
    ];
    for source in cases {
        assert!(EngineConfig::parse(source).is_err(), "{source:?} must fail");
    }
}

#[test]
fn duplicate_keys_are_rejected() {
    let source = "schema: soma.engine/1\nschema: soma.engine/1\n";
    assert!(EngineConfig::parse(source).is_err());
    let source = "schema: soma.engine/1\nvertical:\n  page_weight: 0\n  page_weight: 1\n";
    assert!(EngineConfig::parse(source).is_err());
}

#[test]
fn the_schema_is_required_and_versioned() {
    assert!(EngineConfig::parse("preset: default-v1\n").is_err());
    assert!(EngineConfig::parse("schema: soma.engine/2\n").is_err());
}

#[test]
fn only_the_paragraph_window_is_accepted() {
    let source = "schema: soma.engine/1\nwindow: page\n";
    let error = EngineConfig::parse(source).expect_err("page window");
    assert_eq!(error.path, "window");
}

#[test]
fn unknown_presets_are_rejected() {
    let source = "schema: soma.engine/1\npreset: house-style\n";
    assert!(EngineConfig::parse(source).is_err());
}

#[test]
fn bad_values_are_rejected_with_a_position() {
    let cases = [
        ("schema: soma.engine/1\nmax_kp_atoms: 0\n", 2),
        ("schema: soma.engine/1\nmax_kp_atoms: many\n", 2),
        ("schema: soma.engine/1\nvertical:\n  widow_penalty: -1\n", 3),
        (
            "schema: soma.engine/1\nvertical:\n  badness_scale: 2000000\n",
            1,
        ),
        (
            "schema: soma.engine/1\nfallback:\n  emergency_stretch: 3px\n",
            3,
        ),
        (
            "schema: soma.engine/1\nfallback:\n  allow_emergency: false\n  allow_forbidden: true\n",
            1,
        ),
    ];
    for (source, line) in cases {
        let error = EngineConfig::parse(source).expect_err(source);
        assert_eq!(error.line, line, "{source:?}: {error}");
        assert!(!error.reason.is_empty());
    }
}

#[test]
fn tab_indentation_is_rejected() {
    let source = "schema: soma.engine/1\nfallback:\n\tallow_emergency: true\n";
    let error = EngineConfig::parse(source).expect_err("tabs");
    assert!(error.reason.contains("tabs"));
}

#[test]
fn indented_keys_without_a_section_are_rejected() {
    let source = "schema: soma.engine/1\n  preset: default-v1\n";
    assert!(EngineConfig::parse(source).is_err());
}

#[test]
fn an_engine_config_lays_out_exactly_like_the_hand_built_parameters() {
    let config = EngineConfig::parse(DEFAULTS).expect("parse");
    let doc = document();
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&doc, &template).expect("styles");
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);

    let from_config = layout_with(
        &doc,
        &styled,
        &metrics,
        &config.break_config(),
        PaginationStrategy::VerticalKp,
        config.vertical(),
    )
    .expect("layout from config");
    let hand_built = layout_with(
        &doc,
        &styled,
        &metrics,
        &BreakConfig::from_params(ParamSet::publish_v1()),
        PaginationStrategy::VerticalKp,
        VerticalParams::default_v1(),
    )
    .expect("layout from defaults");

    assert_eq!(from_config.canonical_json(), hand_built.canonical_json());
    assert_eq!(from_config.metrics_hash, table.hash());
    assert!(from_config.page_count() >= 1);
}

#[test]
fn a_vertical_override_reaches_the_layout() {
    let source = DEFAULTS.replace("widow_penalty: 10000", "widow_penalty: 0");
    let config = EngineConfig::parse(&source).expect("parse");
    let doc = document();
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&doc, &template).expect("styles");
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);

    let layout = layout_with(
        &doc,
        &styled,
        &metrics,
        &config.break_config(),
        PaginationStrategy::VerticalKp,
        config.vertical(),
    )
    .expect("layout");
    assert_eq!(layout.vertical_hash, config.vertical().hash());
    assert_ne!(
        layout.vertical_hash,
        VerticalParams::default_v1().hash(),
        "the result must record the non-default vertical model"
    );
}
