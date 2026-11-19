//! Gate C: the renderer must never emit active content or external references.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::Paragraph;
use soma_doc::inline::{Image, Link};
use soma_doc::meta::{DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_layout::layout;
use soma_render_html::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: safety
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;

fn render_document(document: &Document) -> String {
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(document, &template).expect("styles");
    let pages = layout(
        document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    render(&pages, styled.table(), &pages.text)
}

fn document_with(inlines: Vec<soma_doc::Inline>) -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
    )));
    document.renumber();
    document
}

fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\n' => {}
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out
}

#[test]
fn raw_html_in_text_is_escaped_not_executed() {
    let document = document_with(vec![Text::new(
        NodeMeta::synthesized(),
        "<script>alert('x')</script><img src=x onerror=alert(1)>",
    )
    .into()]);
    let html = render_document(&document);

    assert!(!html.contains("<script"), "script tag leaked: {html}");
    assert!(!html.contains("onerror="), "handler leaked");
    assert!(!html.contains("<img"), "raw img leaked");
    assert!(
        text_of(&html).contains("&lt;script&gt;"),
        "text must be escaped: {html}"
    );
}

#[test]
fn links_and_images_do_not_create_external_references() {
    let document = document_with(vec![
        Text::new(NodeMeta::synthesized(), "见 ").into(),
        Link::new(
            NodeMeta::synthesized(),
            "https://example.com/a?b=c",
            vec![Text::new(NodeMeta::synthesized(), "链接").into()],
        )
        .into(),
        Text::new(NodeMeta::synthesized(), " 与 ").into(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef::new("https://example.com/pixel.gif"),
        )
        .into(),
    ]);
    let html = render_document(&document);

    assert!(!html.contains("href="), "the renderer must not emit links");
    assert!(
        !html.contains("src="),
        "the renderer must not emit image sources"
    );
    assert!(
        !html.contains("https://example.com"),
        "URL leaked into output"
    );
    assert!(
        text_of(&html).contains("链接"),
        "link text must still be drawn"
    );
}

#[test]
fn output_contains_no_style_or_script_blocks_from_content() {
    let document = document_with(vec![Text::new(
        NodeMeta::synthesized(),
        "<style>body{display:none}</style>",
    )
    .into()]);
    let html = render_document(&document);

    // The only `<style>` in the document is the renderer's own sheet.
    assert_eq!(html.matches("<style>").count(), 1);
    assert!(!html.contains("display:none"));
}
