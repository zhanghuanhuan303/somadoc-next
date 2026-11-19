//! A5/A6 end-to-end: Markdown → DocIR → styles → pages → HTML.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_layout::layout;
use soma_md::parse_markdown;
use soma_render_html::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: pipeline
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
      family: serif
      color: 202020
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: heading
    declarations:
      font-size: 16pt
      space-before: 1em
      space-after: 0.5em
      keep-with-next: true
  - selector:
      element: strong
    declarations:
      weight: bold
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

const SOURCE: &str = "\
# 第一章

中文排版 **加粗** 与 `code`，以及脚注[^1]。

- 第一项
- 第二项

> 引用一段话。

```rust
let x = 1;
```
";

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
fn markdown_reaches_html() {
    let document = parse_markdown(&format!("{SOURCE}\n[^1]: 脚注内容。\n"));
    document.validate().expect("valid DocIR");

    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");

    assert!(pages.page_count() >= 1);
    let html = render(&pages, styled.table(), &pages.text);
    let visible = text_of(&html);

    assert!(visible.contains("第一章"), "{visible}");
    assert!(visible.contains("中文排版"));
    assert!(visible.contains("加粗"));
    assert!(visible.contains("第一项"));
    assert!(visible.contains("引用一段话。"));
    assert!(visible.contains("脚注内容。"));
    assert!(html.contains("font-weight:bold"));
    assert!(html.contains("data-page=\"1\""));
    assert!(!html.contains('\u{e000}'), "marker placeholder leaked");
    assert!(!html.contains("<br"), "the renderer must not re-break");
}

#[test]
fn the_pipeline_is_byte_stable() {
    let document = parse_markdown(SOURCE);
    let template = parse_template(TEMPLATE).expect("template");

    let run = || {
        let document = parse_markdown(SOURCE);
        let styled = resolve(&document, &template).expect("styles");
        let pages = layout(
            &document,
            &styled,
            &SyntheticMetrics::default(),
            &BreakConfig::default(),
        )
        .expect("layout");
        (document.content_hash(), styled.style_hash(), pages.hash())
    };

    assert_eq!(run(), run());
    assert_eq!(
        document.content_hash(),
        parse_markdown(SOURCE).content_hash()
    );
}

#[test]
fn long_markdown_paginates() {
    let mut source = String::new();
    for index in 0..20 {
        source.push_str(&format!(
            "## 小节 {index}\n\n第{index}段。中文排版需要断行、对齐与标点处理，这一段足够长以占用多行。\n\n"
        ));
    }
    let document = parse_markdown(&source);
    document.validate().expect("valid DocIR");

    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");

    assert!(pages.page_count() >= 2, "content must paginate");

    let html = render(&pages, styled.table(), &pages.text);
    assert!(html.contains("data-page=\"2\""));
}
