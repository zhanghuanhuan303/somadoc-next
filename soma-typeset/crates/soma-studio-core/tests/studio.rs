//! Studio's shared build: fragments, page count and the font-probe samples.
//!
//! These are the seams the browser and the Axum server both consume. The test
//! is native-only; it proves the build output the wasm UI mounts and the
//! `/api/verify` summary are the same shape.

use soma_render_html::PAGE_STYLE;
use soma_studio_core::{build, parse_document, BuildOptions, BuildRequest, SourceFormat};

fn source() -> String {
    let mut text = String::from("# 混排样张\n\n");
    for index in 0..24 {
        text.push_str(&format!(
            "第 {index} 段：中文排版需要处理标点挤压、行边裁剪与中英边界，\
             Latin words like deterministic layout stay on the baseline. \
             这一行足够长，用来跨过多个行盒，并让页面切分发生。\n\n"
        ));
    }
    text
}

fn built() -> soma_studio_core::Build {
    let template = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/templates/plex.yaml"
    ))
    .expect("plex template");
    let parsed = parse_document(SourceFormat::Markdown, source().as_bytes()).expect("parse");
    build(
        parsed,
        BuildOptions {
            template: &template,
            engine: None,
            metrics: None,
            hyphenation: None,
        },
    )
    .expect("build")
}

#[test]
fn a_build_request_defaults_to_the_project_frontend() {
    let legacy = BuildRequest::default();
    assert_eq!(legacy.format, None);
    assert_eq!(legacy.format_or(SourceFormat::Latex), SourceFormat::Latex);

    let html = BuildRequest {
        format: Some(SourceFormat::Html),
        ..BuildRequest::default()
    };
    assert_eq!(html.format_or(SourceFormat::Latex), SourceFormat::Html);
}

#[test]
fn fragments_follow_pages_and_join_the_same_document() {
    let build = built();
    let pages = build.pages.page_count() as usize;
    assert!(pages > 0, "the fixture should paginate");
    let fragments = build.fragments(&soma_studio_core::ImageBytes::new());
    assert_eq!(fragments.len(), pages);
    for (index, fragment) in fragments.iter().enumerate() {
        assert_eq!(fragment.style, PAGE_STYLE);
        assert!(
            fragment
                .body
                .contains(&format!("data-page=\"{}\"", index + 1)),
            "fragment {index} must carry its page number"
        );
    }

    let mut joined = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n<style>");
    joined.push_str(PAGE_STYLE);
    joined.push_str("</style></head><body>\n");
    for fragment in &fragments {
        joined.push_str(&fragment.body);
    }
    joined.push_str("</body></html>\n");
    assert_eq!(
        joined,
        soma_render_html::render(&build.pages, build.styled.table(), &build.pages.text)
    );
}

#[test]
fn probe_samples_come_from_the_layout_faces() {
    let build = built();
    assert!(!build.probe.is_empty(), "the probe needs samples");
    for sample in &build.probe {
        assert!(!sample.text.is_empty());
        assert!(sample.advance > 0, "{} has no advance", sample.text);
        assert!(matches!(
            sample.role.as_str(),
            "serif" | "sans" | "mono" | "fallback"
        ));
    }
    assert!(
        build.probe.iter().any(|sample| sample.text == "中"),
        "the probe must include CJK"
    );
}

#[test]
fn hyphenation_patterns_reach_the_shared_build() {
    let template = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/templates/plex.yaml"
    ))
    .expect("plex template");
    let patterns = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/book/hyphen-en.pat"
    ))
    .expect("english patterns");
    let source = "A line of deliberately long English is placed here to exercise \
        hyphenationhyphenationhyphenationhyphenationhyphenationhyphenation, and then it stops.";
    let parsed = parse_document(SourceFormat::Markdown, source.as_bytes()).expect("parse");
    let build = build(
        parsed,
        BuildOptions {
            template: &template,
            engine: None,
            metrics: None,
            hyphenation: Some(&patterns),
        },
    )
    .expect("build");

    let mut hyphenated = 0;
    let mut overfull = 0;
    for page in &build.pages.pages {
        for block in &page.blocks {
            for placed in &block.lines {
                hyphenated += usize::from(placed.line.hyphenated);
                overfull += usize::from(placed.line.overfull);
            }
        }
    }
    assert!(
        hyphenated > 0,
        "the long word must break at a pattern point"
    );
    assert_eq!(
        overfull, 0,
        "hyphenation must keep every line inside the measure"
    );
}

#[test]
fn build_is_deterministic() {
    let first = built();
    let second = built();
    let images = soma_studio_core::ImageBytes::new();
    assert_eq!(first.fragments(&images), second.fragments(&images));
    assert_eq!(
        first.summary(0).hash,
        second.summary(0).hash,
        "same input must produce the same canonical hash"
    );
}
