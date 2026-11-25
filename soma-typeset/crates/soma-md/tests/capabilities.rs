//! Gate C: the capability matrix must be complete, documented, and honest.
//!
//! "Degraded" is a promise: the content survives as text. Each degraded case has
//! an assertion here that the source text is still present in the plain-text
//! projection of the parsed document.

use soma_doc::Block;
use soma_md::{capability, with_status, CapabilityStatus, CAPABILITIES};
use std::collections::BTreeSet;

const DOCS: &str = include_str!("../../../docs/capabilities.md");

#[test]
fn capability_ids_are_unique_and_non_empty() {
    let mut seen = BTreeSet::new();
    for entry in CAPABILITIES {
        assert!(!entry.syntax.is_empty());
        assert!(!entry.note.is_empty());
        assert!(seen.insert(entry.syntax), "duplicate id {}", entry.syntax);
    }
}

#[test]
fn every_status_has_entries() {
    assert!(with_status(CapabilityStatus::Supported).count() >= 15);
    // The honest set: constructs that survive as text but lose structure.
    assert!(with_status(CapabilityStatus::Degraded).count() >= 1);
    // Rejected is allowed to be empty: a forgiving front end keeps text.
    let _ = with_status(CapabilityStatus::Rejected).count();
}

#[test]
fn every_capability_is_documented() {
    for entry in CAPABILITIES {
        let needle = format!("`{}`", entry.syntax);
        assert!(
            DOCS.contains(&needle),
            "docs/capabilities.md is missing {}",
            entry.syntax
        );
        let status = format!("| `{}`", entry.syntax);
        let line = DOCS
            .lines()
            .find(|line| line.starts_with(&status))
            .unwrap_or_else(|| panic!("missing table row for {}", entry.syntax));
        assert!(
            line.contains(entry.status.as_str()) || line.contains(note_marker(entry.status)),
            "status mismatch for {}: {line}",
            entry.syntax
        );
    }
}

fn note_marker(status: CapabilityStatus) -> &'static str {
    match status {
        CapabilityStatus::Supported => "supported",
        CapabilityStatus::Degraded => "degraded",
        CapabilityStatus::Rejected => "rejected",
    }
}

#[test]
fn the_documented_ids_all_exist_in_code() {
    for line in DOCS.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some(end) = rest.find('`') else {
            continue;
        };
        let id = &rest[..end];
        assert!(
            capability(id).is_some(),
            "docs mention {id}, but the code does not declare it"
        );
    }
}

fn plain_text_of(source: &str) -> String {
    let document = soma_md::parse_markdown(source);
    document.plain_text()
}

#[test]
fn degraded_constructs_keep_their_text() {
    let cases: &[(&str, &str, &str)] =
        &[("unterminated_code_span", "an `open span\n", "`open span")];

    for (syntax, source, expected) in cases {
        let capability = capability(syntax).expect("declared capability");
        assert_eq!(capability.status, CapabilityStatus::Degraded);
        let text = plain_text_of(source);
        assert!(
            text.contains(expected),
            "{syntax} lost its text: {text:?} does not contain {expected:?}"
        );
    }
}

#[test]
fn supported_constructs_produce_structure() {
    let document = soma_md::parse_markdown(
        "# 标题\n\n正文 **粗** `code` [链接](x) ![图](a.png) [[ruby:汉字|hànzì]]\n\n- 项\n  - 子项\n\n> 引用\n\n---\n\n```rust\nx\n```\n\n集注\n===\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n",
    );
    document.validate().expect("valid DocIR");

    assert!(matches!(document.blocks[0], Block::Heading(_)));
    assert!(matches!(document.blocks[1], Block::Paragraph(_)));
    assert!(matches!(document.blocks[2], Block::List(_)));
    assert!(matches!(document.blocks[3], Block::BlockQuote(_)));
    assert!(matches!(document.blocks[4], Block::ThematicBreak(_)));
    assert!(matches!(document.blocks[5], Block::CodeBlock(_)));
    assert!(matches!(document.blocks[6], Block::Heading(_)), "setext");
    assert!(matches!(document.blocks[7], Block::Table(_)), "GFM table");
    let Block::List(list) = &document.blocks[2] else {
        unreachable!()
    };
    assert!(
        list.items[0]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List(_))),
        "nested list"
    );
    let Block::Paragraph(paragraph) = &document.blocks[1] else {
        unreachable!()
    };
    assert!(
        paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, soma_doc::Inline::Ruby(_))),
        "ruby"
    );
}

#[test]
fn the_matrix_is_stable() {
    // Pinning the count makes an accidental silent removal visible.
    assert_eq!(CAPABILITIES.len(), 30);
}

#[test]
fn inline_html_is_a_raw_inline_with_a_text_fallback() {
    let document = soma_md::parse_markdown("text <b>bold</b> end\n");
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, soma_doc::Inline::Raw(_))));
    assert_eq!(document.plain_text().trim(), "text <b>bold</b> end");
}

#[test]
fn an_html_block_is_a_raw_block_with_a_text_fallback() {
    let document = soma_md::parse_markdown("<div class=\"x\">\nkept\n</div>\n\n正文\n");
    assert!(matches!(document.blocks[0], Block::Raw(_)));
    assert!(document.plain_text().contains("<div class=\"x\">"));
    assert!(document.plain_text().contains("正文"));
}
