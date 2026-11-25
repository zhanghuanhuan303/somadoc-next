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
fn definition_lists_become_unordered_lists_without_losing_terms() {
    let html = r#"<dl>
  <dt>术语</dt>
  <dd>第一条定义。</dd>
  <dt>甲</dt>
  <dd><p>第一段</p><p>第二段</p></dd>
</dl>"#;
    let document = parse_html(html);
    let Block::List(list) = &document.blocks[0] else {
        panic!("a dl becomes a list: {:?}", document.blocks[0]);
    };
    assert!(!list.ordered);
    assert_eq!(list.start, 1);
    assert_eq!(list.items.len(), 4);
    assert_eq!(paragraph_text(&list.items[0].blocks[0]), "术语");
    assert_eq!(paragraph_text(&list.items[1].blocks[0]), "第一条定义。");
    assert_eq!(paragraph_text(&list.items[2].blocks[0]), "甲");
    assert_eq!(paragraph_text(&list.items[3].blocks[0]), "第一段");
    assert_eq!(paragraph_text(&list.items[3].blocks[1]), "第二段");
    document.validate().expect("valid DocIR");
}

#[test]
fn nested_definition_lists_and_div_groups_keep_their_structure() {
    let html = r#"<dl><div>
  <dt>外术语</dt>
  <dd>
    <dl><dt>内术语</dt><dd>内定义</dd></dl>
  </dd>
</div></dl>"#;
    let document = parse_html(html);
    let Block::List(outer) = &document.blocks[0] else {
        panic!("a dl becomes a list");
    };
    assert_eq!(outer.items.len(), 2);
    assert_eq!(paragraph_text(&outer.items[0].blocks[0]), "外术语");
    let Block::List(inner) = &outer.items[1].blocks[0] else {
        panic!("the nested dl stays a list: {:?}", outer.items[1].blocks[0]);
    };
    assert_eq!(inner.items.len(), 2);
    assert_eq!(paragraph_text(&inner.items[0].blocks[0]), "内术语");
    assert_eq!(paragraph_text(&inner.items[1].blocks[0]), "内定义");
    document.validate().expect("valid DocIR");
}

#[test]
fn consecutive_terms_and_definitions_are_siblings() {
    let html = "<dl><dt>甲<dt>乙<dd>丙<dd>丁</dl>";
    let document = parse_html(html);
    let Block::List(list) = &document.blocks[0] else {
        panic!("a dl becomes a list");
    };
    assert_eq!(
        list.items.len(),
        4,
        "unclosed dt/dd auto-close on a sibling"
    );
    let text: Vec<String> = list
        .items
        .iter()
        .map(|item| paragraph_text(&item.blocks[0]))
        .collect();
    assert_eq!(text, vec!["甲", "乙", "丙", "丁"]);
    document.validate().expect("valid DocIR");
}

#[test]
fn del_and_ins_map_to_strikethrough_and_underline() {
    let document = parse_html("<p><del>删除</del><ins>插入</ins></p>");
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(matches!(paragraph.inlines[0], Inline::Strikethrough(_)));
    assert!(matches!(paragraph.inlines[1], Inline::Underline(_)));
    assert_eq!(paragraph_text(&document.blocks[0]), "删除插入");
    document.validate().expect("valid DocIR");
}

#[test]
fn del_and_ins_nest_inside_other_inlines() {
    let html = r#"<p><strong><del>粗删</del></strong><em><ins>斜插</ins></em></p>"#;
    let document = parse_html(html);
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(matches!(
        &paragraph.inlines[0],
        Inline::Strong(container)
            if matches!(container.children.as_slice(), [Inline::Strikethrough(_)])
    ));
    assert!(matches!(
        &paragraph.inlines[1],
        Inline::Emphasis(container)
            if matches!(container.children.as_slice(), [Inline::Underline(_)])
    ));
    assert_eq!(paragraph_text(&document.blocks[0]), "粗删斜插");
    document.validate().expect("valid DocIR");
}

#[test]
fn sup_and_sub_stay_semantic_inlines_when_nested() {
    let html = "<p>H<sub>2</sub>O 与 x<sup><em>2</em></sup></p>";
    let document = parse_html(html);
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Subscript(_))));
    let superscript = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Superscript(container) => Some(container),
            _ => None,
        })
        .expect("a superscript");
    assert!(
        matches!(superscript.children.as_slice(), [Inline::Emphasis(_)]),
        "emphasis nests inside superscript"
    );
    assert_eq!(paragraph_text(&document.blocks[0]), "H2O 与 x2");
    document.validate().expect("valid DocIR");
}

#[test]
fn hr_inside_a_container_is_a_thematic_break() {
    let document = parse_html("<blockquote><p>前</p><hr><p>后</p></blockquote>");
    let Block::BlockQuote(quote) = &document.blocks[0] else {
        panic!("expected a blockquote");
    };
    assert_eq!(quote.blocks.len(), 3);
    assert!(matches!(quote.blocks[1], Block::ThematicBreak(_)));
    assert_eq!(paragraph_text(&quote.blocks[0]), "前");
    assert_eq!(paragraph_text(&quote.blocks[2]), "后");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_figure_with_an_image_and_caption_maps_to_the_figure_block() {
    let html = r#"<figure><img src="chart.png" alt="图表"><figcaption>图 <strong>一</strong></figcaption></figure>"#;
    let document = parse_html(html);
    let Block::Figure(figure) = &document.blocks[0] else {
        panic!("expected a figure: {:?}", document.blocks[0]);
    };
    assert_eq!(figure.image.source.src, "chart.png");
    assert_eq!(figure.image.source.alt.as_deref(), Some("图表"));
    let mut caption = String::new();
    collect_inline_text(&figure.caption, &mut caption);
    assert_eq!(caption, "图 一");
    assert!(figure
        .caption
        .iter()
        .any(|inline| matches!(inline, Inline::Strong(_))));
    document.validate().expect("valid DocIR");
}

#[test]
fn a_figure_keeps_content_beside_the_image_and_caption() {
    let html =
        r#"<figure><img src="a.png" alt="A"><p>图旁正文</p><figcaption>标题</figcaption></figure>"#;
    let document = parse_html(html);
    assert!(matches!(document.blocks[0], Block::Figure(_)));
    assert_eq!(paragraph_text(&document.blocks[1]), "图旁正文");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_figure_without_an_image_keeps_its_text() {
    let html = "<figure><figcaption>无图说明</figcaption><p>图内正文</p></figure>";
    let document = parse_html(html);
    let text: Vec<String> = document.blocks.iter().map(paragraph_text).collect();
    assert_eq!(text, vec!["无图说明", "图内正文"]);
    assert!(!document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Figure(_))));
    document.validate().expect("valid DocIR");
}

#[test]
fn new_elements_never_carry_scripts_into_docir() {
    let html = r#"<dl><dt><script>alert('term')</script>术语</dt><dd><ins>定义</ins></dd></dl>"#;
    let document = parse_html(html);
    let canonical = document.canonical_json();
    assert!(!canonical.contains("alert"), "script content is dropped");
    assert!(canonical.contains("术语"), "the term survives");
    assert!(canonical.contains("定义"), "the definition survives");
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
    assert!(first.starts_with("{\"schema\":\"soma.doc/3\""));
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

#[test]
fn pre_mermaid_class_becomes_a_diagram() {
    let document = parse_html("<pre class=\"mermaid\">flowchart TD\n  A --> B</pre>");
    assert!(matches!(document.blocks[0], Block::Diagram(_)));
}
