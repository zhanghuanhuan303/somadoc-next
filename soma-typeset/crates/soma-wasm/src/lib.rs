//! Cross-target fixture: build a fixed DocIR document and a fixed hard-broken,
//! punctuation-heavy paragraph, then print both canonical JSON forms.
//!
//! The same binary runs natively and under `wasm32-wasip1`. The hash gate
//! compares the two byte streams; no floating point or platform API is used.

use soma_core::{break_paragraph, BreakConfig, StyleId, SyntheticMetrics, Width};
use soma_doc::block::{Heading, Paragraph};
use soma_doc::inline::{Container, FootnoteRef};
use soma_doc::meta::{DocumentMeta, TemplateRef};
use soma_doc::node::NodeMeta;
use soma_doc::{
    Block, Document, FootnoteDef, Inline, SourceId, SourceInfo, SourceKind, SourceSpan,
};
use soma_doc::{SourceMap, Text};
use soma_layout::layout;
use soma_md::parse_markdown;
use soma_rules::{justified_style, paragraph_from_lines};
use soma_template::{parse_template, resolve};

pub const LINES: &[&str] = &[
    "中文English混排，数字123。",
    "「引号」，（括号）；冒号：逗号，句号。",
    "排版Engine v2：先规则，后代价。",
];

/// Fixed DocIR fixture. Covers a heading, inline containers, a footnote, and a
/// source map entry so the canonical writer exercises more than one node shape.
pub fn fixture_document() -> Document {
    let source = SourceId(1);
    let span = |start: u32, end: u32| SourceSpan::at(source, start, end);

    let mut source_map = SourceMap::new();
    source_map.push(SourceInfo::new(source, "fixture.md", SourceKind::Markdown));

    let mut document = Document::new(
        DocumentMeta::new()
            .with_title(vec![Text::new(NodeMeta::at(span(0, 4)), "标题").into()])
            .with_language("zh-Hans")
            .with_template(TemplateRef::new("default").with_version("1")),
    );
    document.source_map = source_map;

    document.push_block(Block::Heading(Heading::new(
        NodeMeta::at(span(0, 4)),
        1,
        vec![Text::new(NodeMeta::at(span(2, 4)), "标题").into()],
    )));

    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::at(span(5, 24)),
        vec![
            Text::new(NodeMeta::at(span(5, 9)), "正文").into(),
            Inline::Strong(Container::new(
                NodeMeta::at(span(9, 13)),
                vec![Text::new(NodeMeta::at(span(11, 13)), "强调").into()],
            )),
            Inline::FootnoteRef(FootnoteRef::new(NodeMeta::at(span(13, 16)), "1")),
        ],
    )));

    document.push_footnote(FootnoteDef::new(
        NodeMeta::at(span(18, 24)),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::at(span(18, 24)),
            vec![Text::new(NodeMeta::at(span(20, 24)), "脚注").into()],
        ))],
    ));

    document.renumber();
    document
}

/// Canonical layout JSON for the inline fixture.
pub fn fixture_layout_json() -> String {
    let style = justified_style(Width::from_em_frac(13, 2));
    let paragraph =
        paragraph_from_lines(LINES, style, StyleId(0)).expect("built-in wasm fixture must compile");
    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("built-in wasm fixture must break");
    result.canonical_json()
}

/// Fixed Markdown source for the full pipeline.
pub const MARKDOWN: &str = "# 标题

正文 **强调**，脚注[^1]。

[^1]: 脚注。
";

/// Fixed template for the full pipeline.
pub const TEMPLATE: &str = r#"
name: fixture
page:
  size: A5
  margin: 12mm
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
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

/// Canonical page JSON for the fixed Markdown fixture.
pub fn fixture_pages_json() -> String {
    let document = parse_markdown(MARKDOWN);
    let template = parse_template(TEMPLATE).expect("built-in template must parse");
    let styled = resolve(&document, &template).expect("built-in styles must resolve");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("built-in pipeline must lay out");
    pages.canonical_json()
}

/// Combined fixture: DocIR, inline layout, and page canonical JSON.
pub fn fixture_json() -> String {
    format!(
        "{{\"docIr\":{},\"layout\":{},\"pages\":{}}}",
        fixture_document().canonical_json(),
        fixture_layout_json(),
        fixture_pages_json()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_deterministic_and_exercises_k5_and_docir() {
        let first = fixture_json();
        let second = fixture_json();
        assert_eq!(first, second);
        assert!(first.contains("\"schema\":\"soma.doc/1\""));
        assert!(first.contains("\"schema\":\"soma.layout/6\""));
        assert!(first.contains("\"forcedEnd\":true"));
        assert!(first.contains("\"footnoteDef\""));
        assert!(first.contains("\"schema\":\"soma.pages/1\""));
    }
}
