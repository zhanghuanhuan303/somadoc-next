//! A1 raw-node tests: nothing is lost, and raw payloads are part of content
//! identity.

mod common;

use soma_doc::block::{Paragraph, RawBlock};
use soma_doc::inline::RawInline;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::raw::{RawCapability, RawContent};
use soma_doc::{Block, Document, Inline, Text};

fn raw_document(content: RawContent) -> Document {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Raw(RawBlock::new(NodeMeta::synthesized(), content)));
    document.renumber();
    document
}

#[test]
fn raw_block_keeps_format_value_and_fallback() {
    let json = raw_document(
        RawContent::new("latex", "\\vspace{1em}")
            .with_fallback("[spacing]")
            .with_capability(RawCapability::Preserved),
    )
    .canonical_json();

    assert!(json.contains(r#""format":"latex""#), "{json}");
    assert!(json.contains(r#""value":"\\vspace{1em}""#), "{json}");
    assert!(json.contains(r#""fallback":"[spacing]""#), "{json}");
    assert!(json.contains(r#""capability":"preserved""#), "{json}");
}

#[test]
fn raw_inline_keeps_capability_and_escaped_payload() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![
            Text::new(NodeMeta::synthesized(), "前").into(),
            Inline::Raw(RawInline::new(
                NodeMeta::synthesized(),
                RawContent::new("html", "<span class=\"x\">旧</span>")
                    .with_fallback("旧")
                    .with_capability(RawCapability::Degraded),
            )),
        ],
    )));
    document.renumber();

    let json = document.canonical_json();
    assert!(json.contains("\"capability\":\"degraded\""), "{json}");
    assert!(
        json.contains(r#""value":"<span class=\"x\">旧</span>""#),
        "raw payload must be preserved and escaped: {json}"
    );
}

#[test]
fn raw_projects_fallback_into_plain_text() {
    let document = raw_document(
        RawContent::new("html", "<hr>")
            .with_fallback("[rule]")
            .with_capability(RawCapability::Degraded),
    );

    assert_eq!(document.plain_text(), "[rule]\n");
}

#[test]
fn raw_payload_changes_the_content_hash() {
    let first = raw_document(RawContent::new("html", "<b>a</b>"));
    let second = raw_document(RawContent::new("html", "<b>b</b>"));

    assert_ne!(first.content_hash(), second.content_hash());
}
