//! A5 renderer: absolute positions, escaping, no re-breaking.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::{Heading, Paragraph};
use soma_doc::inline::FootnoteRef;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, FootnoteDef, Text};
use soma_layout::{layout, FOOTNOTE_MARKER};
use soma_render_html::{render, render_fragments, render_page, PAGE_STYLE};
use soma_template::{parse_template, resolve, StyledDoc, Template};

const TEMPLATE: &str = r#"
name: render
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
      family: serif
      color: 222222
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: strong
    declarations:
      weight: bold
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

fn template() -> Template {
    parse_template(TEMPLATE).expect("template")
}

fn styled(document: &Document) -> StyledDoc {
    resolve(document, &template()).expect("styles")
}

fn document() -> Document {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![
            Text::new(NodeMeta::synthesized(), "正文 ").into(),
            soma_doc::Inline::Strong(soma_doc::Container::new(
                NodeMeta::synthesized(),
                vec![Text::new(NodeMeta::synthesized(), "加粗").into()],
            )),
            FootnoteRef::new(NodeMeta::synthesized(), "1").into(),
            Text::new(NodeMeta::synthesized(), " 结束。").into(),
        ],
    )));
    document.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), "脚注内容。").into()],
        ))],
    ));
    document.renumber();
    document
}

fn render_document() -> String {
    let document = document();
    let styled = styled(&document);
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    render(&pages, styled.table(), &pages.text)
}

/// Visible text with tags removed; atoms are separate spans, so assertions on
/// running text must go through this.
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
fn page_style_has_a_light_fallback_color() {
    // The proof is mounted in a dark UI; a missing template color must not
    // inherit `canvastext` (white) on a white page.
    assert!(PAGE_STYLE.contains("color-scheme:light"));
    assert!(PAGE_STYLE.contains("color:#111"));
}

#[test]
fn renders_pages_with_absolute_positions() {
    let html = render_document();

    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("data-page=\"1\""));
    assert!(html.contains("position:absolute"));
    assert!(html.contains("left:"));
    assert!(html.contains("em;"));
    assert!(!html.contains("<br"), "the renderer must not re-break");
    assert!(!html.to_lowercase().contains("<script"));
}

#[test]
fn heading_line_height_is_a_ratio_of_its_font() {
    const HEADING_TEMPLATE: &str = r#"
name: heading
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: heading
    declarations:
      font-size: 2em
      line-height: 1.2
"#;
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        1,
        vec![Text::new(NodeMeta::synthesized(), "标题").into()],
    )));
    document.renumber();
    let template = parse_template(HEADING_TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render(&pages, styled.table(), &pages.text);
    assert!(html.contains("font-size:2.000em"), "{html}");
    assert!(html.contains("line-height:1.199em"), "{html}");
    assert!(!html.contains("line-height:2.400em"), "{html}");
}

#[test]
fn text_and_styles_are_applied() {
    let html = render_document();

    let text = text_of(&html);
    assert!(text.contains("正文"), "{text}");
    assert!(text.contains("加粗"));
    assert!(html.contains("font-weight:bold"));
    assert!(html.contains("font-family:serif"));
    assert!(html.contains("color:#222222"));
    assert!(html.contains("font-size:1.000em") || html.contains("font-size:"));
}

#[test]
fn footnote_markers_render_the_number() {
    let html = render_document();
    let marker = FOOTNOTE_MARKER.encode_utf8(&mut [0u8; 4]).to_string();

    assert!(
        !html.contains(&marker),
        "placeholder must not reach the output"
    );
    assert!(
        html.contains(">1</span>"),
        "the marker must draw its number"
    );
    assert!(text_of(&html).contains("脚注内容。"));
}

#[test]
fn html_is_escaped() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), "a < b & c > d \"q\"").into()],
    )));
    document.renumber();

    let styled = styled(&document);
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render(&pages, styled.table(), &pages.text);

    let visible = text_of(&html);
    // Spaces are glue, not atoms: the gap is positional, so the text stream has
    // no space between the segmented characters.
    assert!(
        visible.contains("a&lt;b&amp;c&gt;d&quot;q&quot;"),
        "{visible}"
    );
    assert!(!visible.contains("< b"));
}

#[test]
fn page_fragments_join_to_the_same_document() {
    let document = document();
    let styled = styled(&document);
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render(&pages, styled.table(), &pages.text);
    let fragments = render_fragments(&pages, styled.table(), &pages.text);
    let mut joined = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n<style>");
    joined.push_str(PAGE_STYLE);
    joined.push_str("</style></head><body>\n");
    for fragment in &fragments {
        assert_eq!(fragment.style, PAGE_STYLE);
        joined.push_str(&fragment.body);
    }
    joined.push_str("</body></html>\n");
    assert_eq!(html, joined);
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(render_document(), render_document());
}

#[test]
fn single_page_rendering_works() {
    let document = document();
    let styled = styled(&document);
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render_page(&pages, styled.table(), &pages.text, 0);

    assert!(html.contains("data-page=\"1\""));
    assert!(!html.contains("data-page=\"2\""));
}
