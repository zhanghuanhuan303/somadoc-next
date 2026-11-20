//! HTML subset front end: DocIR shapes, degradation, recovery, pipeline.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::inline::Inline;
use soma_doc::Block;
use soma_html::{parse_html, parse_html_with_meta};
use soma_layout::layout;
use soma_render_html::render;
use soma_template::{parse_template, resolve};

fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\n' | '\r' => {}
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out
}

fn paragraph_text(block: &Block) -> String {
    let Block::Paragraph(paragraph) = block else {
        panic!("not a paragraph: {block:?}");
    };
    let mut out = String::new();
    collect_inline_text(&paragraph.inlines, &mut out);
    out
}

fn collect_inline_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&text.value),
            Inline::Code(code) => out.push_str(&code.value),
            Inline::Strong(children)
            | Inline::Emphasis(children)
            | Inline::Underline(children)
            | Inline::Strikethrough(children)
            | Inline::SmallCaps(children)
            | Inline::Superscript(children)
            | Inline::Subscript(children) => collect_inline_text(&children.children, out),
            Inline::Link(link) => collect_inline_text(&link.children, out),
            Inline::Ruby(ruby) => {
                out.push_str(&ruby.base);
                out.push_str(&ruby.annotation);
            }
            Inline::LineBreak(_) => out.push(' '),
            _ => {}
        }
    }
}

#[test]
fn headings_paragraphs_and_inlines_map_to_docir() {
    let html = r#"<h1>第一章</h1><p>正文 <strong>加粗</strong>，<em>斜体</em>，<code>let x = 1;</code>，<a href="https://example.com">链接</a>，<u>下划线</u>，<s>删除</s>，<sup>上标</sup><sub>下标</sub>。</p>"#;
    let document = parse_html(html);
    assert_eq!(document.blocks.len(), 2);
    assert!(matches!(document.blocks[0], Block::Heading(_)));

    let Block::Paragraph(paragraph) = &document.blocks[1] else {
        panic!("expected paragraph");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Strong(_))));
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Emphasis(_))));
    assert!(paragraph.inlines.iter().any(|inline| match inline {
        Inline::Link(link) => link.href == "https://example.com",
        _ => false,
    }));
    assert_eq!(
        paragraph_text(&document.blocks[1]),
        "正文 加粗，斜体，let x = 1;，链接，下划线，删除，上标下标。"
    );
    document.validate().expect("valid DocIR");
}

#[test]
fn lists_quotes_and_pre_blocks_map_to_docir() {
    let html = r#"<ul><li>第一项</li><li><p>第二项</p></li></ul>
<ol start="3"><li>第三项</li></ol>
<blockquote><p>引用一段话。</p></blockquote>
<pre class="language-rust"><code>let x = 1;
let y = 2;</code></pre>
<hr>"#;
    let document = parse_html(html);
    assert!(matches!(document.blocks[0], Block::List(_)));
    assert!(matches!(document.blocks[1], Block::List(_)));
    assert!(matches!(document.blocks[2], Block::BlockQuote(_)));
    assert!(matches!(document.blocks[3], Block::CodeBlock(_)));
    assert!(matches!(document.blocks[4], Block::ThematicBreak(_)));

    let Block::List(list) = &document.blocks[1] else {
        panic!("ordered list");
    };
    assert!(list.ordered);
    assert_eq!(list.start, 3);
    assert_eq!(list.items.len(), 1);

    let Block::CodeBlock(code) = &document.blocks[3] else {
        panic!("code block");
    };
    assert_eq!(code.lang.as_deref(), Some("rust"));
    assert_eq!(code.value, "let x = 1;\nlet y = 2;");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_table_becomes_a_full_grid() {
    let html = r#"<table>
  <thead><tr><th>项目</th><th>金额</th><th>备注</th></tr></thead>
  <tbody>
    <tr><td colspan="2">合计</td><td rowspan="2">跨两行</td></tr>
    <tr><td>甲</td><td>乙</td></tr>
  </tbody>
</table>"#;
    let document = parse_html(html);
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header);
    assert!(!table.rows[1].header);
    assert_eq!(table.rows[1].cells[0].colspan, 2);
    assert_eq!(table.rows[1].cells[1].rowspan, 2);
    // The second body row is padded so the grid is full.
    assert_eq!(table.rows[2].cells.len(), 2);
    document.validate().expect("valid DocIR");
}

#[test]
fn a_colgroup_supplies_relative_column_widths() {
    let html = r#"<table>
  <colgroup>
    <col width="25%">
    <col span="2" width="100">
  </colgroup>
  <tr><th>甲</th><th>乙</th><th>丙</th></tr>
  <tr><td>1</td><td>2</td><td>3</td></tr>
</table>"#;
    let document = parse_html(html);
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.columns[0].share, 250);
    assert_eq!(table.columns[1].share, 100);
    assert_eq!(table.columns[2].share, 100, "span repeats the hint");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_table_without_colgroup_has_no_width_hints() {
    let document = parse_html("<table><tr><td>甲</td></tr></table>");
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(table.columns[0].share, 0);
}

#[test]
fn a_table_caption_becomes_the_table_title() {
    let html = r#"<table>
  <caption>支出<strong>明细</strong></caption>
  <tr><th>项目</th><th>金额</th></tr>
  <tr><td>甲</td><td>1</td></tr>
</table>"#;
    let document = parse_html(html);
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert!(!table.caption.is_empty(), "the caption is kept");
    document.validate().expect("valid DocIR");
}

#[test]
fn unknown_tags_keep_their_text_and_scripts_are_dropped() {
    let html = r#"<p>前 <custom>未知标签</custom> 后</p>
<script>alert('x')</script>
<style>.x{color:red}</style>
<div><span class="note">容器</span></div>"#;
    let document = parse_html(html);
    let text: Vec<String> = document.blocks.iter().map(paragraph_text).collect();
    assert_eq!(text, vec!["前 未知标签 后", "容器"]);
    let canonical = document.canonical_json();
    assert!(!canonical.contains("alert"));
    assert!(!canonical.contains("color:red"));
    document.validate().expect("valid DocIR");
}

#[test]
fn entities_and_numeric_references_decode() {
    let html = "<p>a &amp; b &lt; c &#65; &#x4E2D; &hellip; &nbsp;d</p>";
    let document = parse_html(html);
    assert_eq!(paragraph_text(&document.blocks[0]), "a & b < c A 中 … d");
}

#[test]
fn malformed_html_is_recovered_without_panicking() {
    let html = "<p>未闭合 <strong>粗体<p>第二段<div><li>散落的项</li></div>";
    let document = parse_html(html);
    document.validate().expect("valid DocIR");
    let text: Vec<String> = document.blocks.iter().map(paragraph_text).collect();
    assert!(text.iter().any(|value| value.contains("未闭合")));
    assert!(text.iter().any(|value| value.contains("粗体")));
    assert!(text.iter().any(|value| value.contains("散落的项")));
}

#[test]
fn the_html_lang_fills_the_document_language() {
    let document = parse_html("<html lang=\"zh-Hans\"><body><p>正文</p></body></html>");
    assert_eq!(document.meta.language.as_deref(), Some("zh-Hans"));

    let mut meta = soma_doc::meta::DocumentMeta::new().with_language("en");
    meta.title = None;
    let document = parse_html_with_meta("<p>Text</p>", meta);
    assert_eq!(document.meta.language.as_deref(), Some("en"));
}

#[test]
fn parsing_is_deterministic() {
    let html = "<h2>标题</h2><p>正文 <strong>重点</strong>。</p>";
    let first = parse_html(html).canonical_json();
    let second = parse_html(html).canonical_json();
    assert_eq!(first, second);
    assert!(first.starts_with("{\"schema\":\"soma.doc/2\""));
}

#[test]
fn html_reaches_the_page_renderer() {
    const TEMPLATE: &str = r#"
name: html
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
      element: heading
    declarations:
      font-size: 16pt
      space-before: 1em
      keep-with-next: true
"#;
    let document = parse_html("<h1>标题</h1><p>正文 <strong>重点</strong>。</p>");
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let metrics = SyntheticMetrics::default();
    let pages = layout(&document, &styled, &metrics, &BreakConfig::default()).expect("layout");
    let html = render(&pages, styled.table(), &pages.text);
    let text = visible_text(&html);
    assert!(text.contains("标题"), "{text}");
    assert!(text.contains("正文"), "{text}");
    assert!(text.contains("重点"), "{text}");
}
