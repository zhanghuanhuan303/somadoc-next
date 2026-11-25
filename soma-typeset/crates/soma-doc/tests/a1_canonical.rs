//! A1 canonical-format tests: byte-level stability, escaping, hash modes.

mod common;

use common::{small_document, text};
use soma_doc::block::Paragraph;
use soma_doc::meta::{DocumentMeta, TemplateRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, IndexRef, Inline, Text};

#[test]
fn small_document_matches_the_golden_bytes() {
    let actual = small_document().canonical_json();
    let golden = include_str!("golden/a1_small.json").trim_end_matches('\n');
    assert_eq!(actual, golden);
}

#[test]
fn canonical_json_is_stable_across_runs() {
    let document = small_document();
    let first = document.canonical_json();
    let second = document.canonical_json();
    assert_eq!(first, second);
    assert_eq!(document.doc_hash(), small_document().doc_hash());
}

#[test]
fn canonical_json_reports_the_node_count() {
    let json = small_document().canonical_json();
    assert!(json.contains("\"nodeCount\":4"), "{json}");
}

#[test]
fn canonical_json_escapes_strings() {
    let value = "a\"b\\c\nd\te\u{1}";
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), value).into()],
    )));
    document.renumber();

    let json = document.canonical_json();
    let expected = r#""text":"a\"b\\c\nd\te\u0001""#;
    assert!(json.contains(expected), "escaping mismatch in {json}");
}

#[test]
fn an_index_marker_round_trips_through_canonical_json() {
    let marker = IndexRef::new(NodeMeta::synthesized(), "排版");
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![
            Text::new(NodeMeta::synthesized(), "正文").into(),
            Inline::Index(marker),
        ],
    )));
    document.renumber();
    document.validate().expect("valid DocIR");

    let json = document.canonical_json();
    assert!(json.contains(r#""kind":"index""#), "{json}");
    assert!(json.contains(r#""term":"排版""#), "{json}");
    assert_eq!(
        document.plain_text().trim_end(),
        "正文",
        "a marker is not text"
    );
}

#[test]
fn an_empty_index_term_is_rejected() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Index(IndexRef::new(NodeMeta::synthesized(), ""))],
    )));
    document.renumber();
    let error = document.validate().expect_err("empty terms are invalid");
    assert!(error.reason.contains("index term"), "{error}");
}

#[test]
fn canonical_json_has_no_float_tokens() {
    let json = small_document().canonical_json();
    let bytes = json.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'.' {
            continue;
        }
        let before = index.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(index + 1).copied();
        let digit_before = before.is_some_and(|b| b.is_ascii_digit());
        let digit_after = after.is_some_and(|b| b.is_ascii_digit());
        assert!(
            !(digit_before && digit_after),
            "canonical output must be integer-only: {json}"
        );
    }
}

#[test]
fn content_mode_normalises_provenance() {
    let document = small_document();
    let full = document.canonical_json();
    let content = document.canonical_content_json();

    assert!(!full.contains("\"id\":0"));
    assert!(content.contains("\"id\":0"));
    assert!(content.contains("\"sourceMap\":[]"));
    assert!(full.contains("\"sourceMap\":[{\"id\":1"));
    assert!(content.contains("\"span\":{\"source\":0,\"start\":0,\"end\":0}"));
}

#[test]
fn the_template_reference_is_content_neutral() {
    let mut plain = Document::new(DocumentMeta::new().with_title(vec![text("标题")]));
    plain.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("正文")],
    )));
    plain.renumber();

    let mut templated = plain.clone();
    templated.meta.template = Some(TemplateRef::new("book").with_version("2"));

    assert_eq!(plain.content_hash(), templated.content_hash());
    assert_ne!(plain.doc_hash(), templated.doc_hash());
}

#[test]
fn content_hash_changes_with_text() {
    let mut first = Document::new(DocumentMeta::new());
    first.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("甲")],
    )));
    first.renumber();

    let mut second = first.clone();
    if let Block::Paragraph(paragraph) = &mut second.blocks[0] {
        paragraph.inlines = vec![text("乙")];
    }
    second.renumber();

    assert_ne!(first.content_hash(), second.content_hash());
}
