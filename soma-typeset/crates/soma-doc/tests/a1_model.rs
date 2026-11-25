//! A1 model tests: node coverage, renumbering, text projection, hashes.

mod common;

use common::{maximal_document, text};
use soma_doc::block::{CodeBlock, Paragraph};
use soma_doc::inline::{FootnoteRef, Image, Ruby};
use soma_doc::meta::{DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, NodeId, Text};

#[test]
fn empty_document_is_valid_and_fully_determined() {
    let document = Document::new(DocumentMeta::new());

    assert_eq!(document.node_count(), 0);
    assert!(document.validate().is_ok());
    assert_eq!(
        document.canonical_json(),
        "{\"schema\":\"soma.doc/2\",\"docIrVersion\":2,\"nodeCount\":0,\
\"meta\":{\"title\":null,\"authors\":[],\"language\":null,\"date\":null,\
\"keywords\":[],\"template\":null,\"cover\":null,\"fields\":[]},\
\"blocks\":[],\"footnotes\":[],\"sourceMap\":[]}"
    );
}

#[test]
fn maximal_document_validates() {
    let document = maximal_document();
    document.validate().expect("maximal document must validate");
    assert!(document.node_count() > 40, "fixture should be large");
}

#[test]
fn canonical_output_covers_every_node_kind() {
    let json = maximal_document().canonical_json();

    let kinds = [
        // blocks
        "paragraph",
        "heading",
        "list",
        "listItem",
        "blockQuote",
        "codeBlock",
        "mathBlock",
        "figure",
        "table",
        "column",
        "row",
        "cell",
        "thematicBreak",
        "pageBreak",
        "section",
        "generated",
        "raw",
        "footnoteDef",
        // inlines
        "text",
        "code",
        "strong",
        "emphasis",
        "underline",
        "strikethrough",
        "smallCaps",
        "superscript",
        "subscript",
        "link",
        "image",
        "math",
        "footnoteRef",
        "ruby",
        "lineBreak",
    ];

    for kind in kinds {
        let needle = format!("\"kind\":\"{kind}\"");
        assert!(json.contains(&needle), "canonical output is missing {kind}");
    }
}

#[test]
fn renumber_is_deterministic_and_contiguous() {
    let mut document = maximal_document();
    let first = document.canonical_json();

    document.renumber();
    let second = document.canonical_json();
    assert_eq!(first, second, "renumber must be idempotent");

    let ids = document.node_ids();
    assert_eq!(ids.len() as u32, document.node_count());
    let expected: Vec<NodeId> = (1..=document.node_count()).map(NodeId::new).collect();
    assert_eq!(ids, expected, "ids must be 1..=node_count in order");
}

#[test]
fn renumber_leaves_no_unassigned_ids() {
    let json = maximal_document().canonical_json();

    // Content mode zeroes ids by design; the full form must never contain one.
    assert!(
        !json.contains("\"id\":0"),
        "a node was not visited by renumber"
    );
}

#[test]
fn plain_text_extracts_text_and_code() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![
            Text::new(NodeMeta::synthesized(), "中文 ").into(),
            soma_doc::Code::new(NodeMeta::synthesized(), "x = 1").into(),
        ],
    )));
    document.push_block(Block::CodeBlock(CodeBlock::new(
        NodeMeta::synthesized(),
        Some("rust".into()),
        "let y = 2;",
    )));
    document.renumber();

    assert_eq!(document.plain_text(), "中文 x = 1\nlet y = 2;\n");
}

#[test]
fn plain_text_uses_image_alt_and_skips_footnote_markers() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![
            Inline::Image(Image::new(
                NodeMeta::synthesized(),
                ImageRef::new("a.png").with_alt("图"),
            )),
            Inline::FootnoteRef(FootnoteRef::new(NodeMeta::synthesized(), "1")),
            Inline::Ruby(Ruby::new(NodeMeta::synthesized(), "漢", "かん")),
        ],
    )));
    document.renumber();

    // Alt text and ruby base survive; the reference marker and the annotation
    // do not appear in plain text.
    assert_eq!(document.plain_text(), "图漢\n");
}

#[test]
fn title_text_is_projected() {
    let mut document = Document::new(DocumentMeta::new().with_title(vec![text("标题")]));
    document.renumber();

    assert_eq!(document.title_text().as_deref(), Some("标题"));
}

#[test]
fn content_hash_ignores_provenance_and_doc_hash_keeps_it() {
    fn build(source_name: &str, base: u32) -> Document {
        let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
        document.source_map.push(soma_doc::SourceInfo::new(
            soma_doc::SourceId(1),
            source_name,
            soma_doc::SourceKind::Markdown,
        ));
        let span = soma_doc::SourceSpan::at(soma_doc::SourceId(1), base, base + 2);
        document.push_block(Block::Paragraph(Paragraph::new(
            NodeMeta::at(span),
            vec![Text::new(NodeMeta::at(span), "内容").into()],
        )));
        document.renumber();
        document
    }

    let first = build("a.md", 0);
    let second = build("b.md", 100);

    assert_eq!(first.content_hash(), second.content_hash());
    assert_ne!(first.doc_hash(), second.doc_hash());

    let mut edited = second.clone();
    if let Block::Paragraph(paragraph) = &mut edited.blocks[0] {
        let span = paragraph.inlines[0].meta().span;
        paragraph.inlines = vec![Text::new(NodeMeta::at(span), "内容改").into()];
    }
    edited.renumber();
    assert_ne!(second.content_hash(), edited.content_hash());
}
