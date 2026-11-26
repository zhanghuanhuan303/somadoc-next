//! Figures in HTML and SVG: an asset that resolves is inlined, and the document
//! never names an external resource (ADR-0048).

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::Figure;
use soma_doc::inline::Image;
use soma_doc::meta::{DocumentMeta, ImageIntrinsic, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document};
use soma_layout::{layout, ImageSource, NoImages, PagedLayout};
use soma_template::{parse_template, resolve, StyledDoc};

const TEMPLATE: &str = r#"
name: figures
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;

/// The smallest thing that claims to be a PNG. The renderers only sniff the
/// signature; decoding is the PDF backend's job.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a png";

struct Assets;

impl ImageSource for Assets {
    fn image(&self, src: &str) -> Option<&[u8]> {
        (src == "fig/chart.png").then_some(PNG)
    }
}

fn fixture() -> (PagedLayout, StyledDoc) {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Figure(Figure::new(
        NodeMeta::synthesized(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef {
                src: String::from("fig/chart.png"),
                alt: Some(String::from("一张图")),
                title: None,
                intrinsic: Some(ImageIntrinsic {
                    width_px: 400,
                    height_px: 200,
                }),
            },
        ),
    )));
    document.renumber();
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    (pages, styled)
}

#[test]
fn html_inlines_a_figure_as_a_data_uri() {
    let (pages, styled) = fixture();
    let html = soma_render_html::render_with(&pages, styled.table(), &pages.text, &Assets);

    assert!(
        html.contains("<img class=\"figure\" alt=\"一张图\""),
        "a resolved figure is an image with its alt text: {html}"
    );
    assert!(
        html.contains("src=\"data:image/png;base64,"),
        "the bytes travel inside the document"
    );
    assert!(
        !html.contains("fig/chart.png"),
        "the source path must not leak into the output"
    );
}

#[test]
fn svg_inlines_a_figure_as_a_data_uri() {
    let (pages, styled) = fixture();
    let svg = soma_render_svg::render_with(&pages, styled.table(), &pages.text, &Assets);

    assert!(
        svg.contains("<image "),
        "a resolved figure is an image: {svg}"
    );
    assert!(svg.contains("href=\"data:image/png;base64,"));
    assert!(svg.contains("preserveAspectRatio=\"none\""));
    assert!(
        !svg.contains("fig/chart.png"),
        "the source path must not leak"
    );
}

#[test]
fn without_assets_both_renderers_keep_the_placeholder() {
    let (pages, styled) = fixture();
    let html = soma_render_html::render(&pages, styled.table(), &pages.text);
    let svg = soma_render_svg::render(&pages, styled.table(), &pages.text);

    assert!(html.contains("<div class=\"figure\""));
    assert!(!html.contains("src="), "no source, no attribute");
    assert!(svg.contains("fill=\"#eee\" stroke=\"#ccc\""));
    assert!(!svg.contains("<image "));
    // The null source is a public value, not a special case in the renderer.
    assert!(NoImages.image("fig/chart.png").is_none());
}
