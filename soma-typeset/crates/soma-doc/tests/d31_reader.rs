//! D31: canonical JSON reader. The writer stays the contract; the reader must
//! return exactly the document the writer serialized.

mod common;

use common::{maximal_document, small_document};
use soma_doc::block::{Column, Paragraph, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};

#[test]
fn reader_round_trips_every_node_kind() {
    let document = maximal_document();
    document.validate().expect("fixture must be valid");

    let json = document.canonical_json();
    let parsed = Document::from_canonical_json(&json).expect("reader accepts writer output");

    assert_eq!(parsed, document, "reader(writer(doc)) must equal doc");
    assert_eq!(
        parsed.canonical_json(),
        json,
        "writer(reader(writer(doc))) must equal writer(doc)"
    );
    assert_eq!(parsed.doc_hash(), document.doc_hash());
    assert_eq!(parsed.content_hash(), document.content_hash());
    assert_eq!(parsed.node_count(), document.node_count());
}

#[test]
fn reader_reads_the_golden_bytes() {
    let golden = include_str!("golden/a1_small.json").trim_end_matches('\n');
    let parsed = Document::from_canonical_json(golden).expect("golden bytes are canonical");

    assert_eq!(parsed, small_document());
    assert_eq!(parsed.canonical_json(), golden);
}

#[test]
fn reader_round_trips_the_content_form() {
    let document = maximal_document();
    let json = document.canonical_content_json();

    let parsed = Document::from_canonical_json(&json).expect("content form is readable");
    assert_eq!(parsed.canonical_content_json(), json);
    assert_eq!(parsed.node_count(), document.node_count());
}

#[test]
fn reader_decodes_escapes_and_surrogate_pairs() {
    let value = "\"\\/\u{8}\u{c}\n\r\t中😀";
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), value).into()],
    )));
    document.renumber();

    let json = document.canonical_json();
    // Canonical output never needs `\/`, `\b` or `\f`, and it writes non-ASCII
    // characters literally. Swap the text payload for the equivalent escaped
    // spellings, including a UTF-16 surrogate pair.
    let canonical = r#""\"\\/\u0008\u000c\n\r\t中😀""#;
    assert!(
        json.contains(canonical),
        "unexpected canonical text: {json}"
    );
    let escaped = r#""\"\\\/\b\f\n\r\t\u4e2d\ud83d\ude00""#;
    let rewritten = json.replace(canonical, escaped);
    assert_ne!(rewritten, json);

    let parsed = Document::from_canonical_json(&rewritten).expect("all JSON escapes decode");
    assert_eq!(parsed, document);
    assert_eq!(parsed.canonical_json(), json);
}

#[test]
fn reader_reads_the_optional_column_share() {
    let mut document = Document::new(DocumentMeta::new());
    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Left).with_share(600)],
        Vec::new(),
    );
    document.push_block(Block::Table(table));
    document.renumber();

    let json = document.canonical_json();
    assert!(
        json.contains("\"align\":\"left\",\"share\":600"),
        "the writer emits valid JSON: {json}"
    );
    let parsed = Document::from_canonical_json(&json).expect("share is readable");
    match &parsed.blocks[0] {
        Block::Table(table) => assert_eq!(table.columns[0].share, 600),
        other => panic!("expected a table, got {other:?}"),
    }
    assert_eq!(parsed.canonical_json(), json);
}

#[test]
fn reader_rejects_unknown_document_fields() {
    let json = small_document().canonical_json();
    let injected = json.replacen("{\"schema\"", "{\"bogus\":0,\"schema\"", 1);

    let error = Document::from_canonical_json(&injected).expect_err("unknown fields are rejected");
    assert_eq!(error.reason, "unknown field");
    assert!(error.path.contains("bogus"), "{error}");
}

#[test]
fn reader_rejects_unknown_node_fields() {
    let json = small_document().canonical_json();
    let injected = json.replacen(
        "\"lang\":null},\"text\":\"你好\"",
        "\"lang\":null},\"bogus\":0,\"text\":\"你好\"",
        1,
    );

    let error = Document::from_canonical_json(&injected).expect_err("unknown fields are rejected");
    assert_eq!(error.reason, "unknown field");
    assert!(error.path.contains("bogus"), "{error}");
}

#[test]
fn reader_rejects_unknown_kinds_and_enums() {
    let json = small_document()
        .canonical_json()
        .replace("\"kind\":\"paragraph\"", "\"kind\":\"bogus\"");
    let error = Document::from_canonical_json(&json).expect_err("unknown kinds are rejected");
    assert_eq!(error.reason, "unknown kind");

    let json = small_document()
        .canonical_json()
        .replace("\"kind\":\"markdown\"", "\"kind\":\"pandoc\"");
    let error =
        Document::from_canonical_json(&json).expect_err("unknown source kinds are rejected");
    assert_eq!(error.reason, "unknown source kind");
}

#[test]
fn reader_rejects_malformed_values() {
    let json =
        small_document()
            .canonical_json()
            .replacen("\"text\":\"你好\"", "\"text\":\"\\q\"", 1);
    let error = Document::from_canonical_json(&json).expect_err("bad escapes are rejected");
    assert_eq!(error.reason, "invalid escape");

    let json =
        small_document()
            .canonical_json()
            .replacen("\"text\":\"你好\"", "\"text\":\"\\ud83d\"", 1);
    let error = Document::from_canonical_json(&json).expect_err("lone surrogates are rejected");
    assert_eq!(error.reason, "invalid surrogate pair");

    let json = small_document()
        .canonical_json()
        .replace("\"nodeCount\":4", "\"nodeCount\":5");
    let error = Document::from_canonical_json(&json).expect_err("nodeCount must match");
    assert_eq!(error.reason, "node count mismatch");

    let json = small_document()
        .canonical_json()
        .replace("\"nodeCount\":4", "\"nodeCount\":999999999999999999999");
    let error = Document::from_canonical_json(&json).expect_err("integers are bounded");
    assert_eq!(error.reason, "integer out of range");

    let json = small_document().canonical_json();
    let error = Document::from_canonical_json(&json[..json.len() - 1]).expect_err("input must end");
    assert_eq!(error.reason, "expected `}`");
}
