//! Hyphenation: long Latin words may break at supplied pattern points, and a
//! broken line ends in a real hyphen (ADR-0059).

mod common;

use common::{document, engine, metrics, paragraph_with, styled, text};
use soma_layout::{layout, layout_with_hyphenation, PagedLayout, TextSource};

const TEMPLATE: &str = r#"
name: hyphen
page:
  size: custom
  width: 26mm
  height: 120mm
  margin: 4mm
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
      first-indent: 0
"#;

/// The classic `hyphenation` example, cut down to the patterns it needs.
const PATTERNS: &str = "\
% a minimal English set
.hy3ph
he2n
hena4
hen5at
n2at
o1t
o1n
2io
1tion
";

fn word() -> Vec<soma_doc::Inline> {
    vec![text("前文 "), text("hyphenation"), text(" 后文")]
}

fn build(inlines: Vec<soma_doc::Inline>, hyphenate: bool) -> PagedLayout {
    let doc = document(vec![paragraph_with(inlines)]);
    let styled = styled(&doc, TEMPLATE);
    let patterns = soma_hyphen::Patterns::parse(PATTERNS);
    if hyphenate {
        layout_with_hyphenation(
            &doc,
            &styled,
            &metrics(),
            &engine(),
            soma_layout::PaginationStrategy::VerticalKp,
            soma_layout::VerticalParams::default_v1(),
            Some(&patterns),
        )
        .expect("layout")
    } else {
        layout(&doc, &styled, &metrics(), &engine()).expect("layout")
    }
}

fn lines_text(layout: &PagedLayout) -> Vec<String> {
    let mut lines = Vec::new();
    for page in &layout.pages {
        for block in &page.blocks {
            for placed in &block.lines {
                let mut text = String::new();
                for atom in &placed.line.boxes {
                    text.push_str(layout.text.atom_text(block.node, atom.atom));
                }
                lines.push(text);
            }
        }
    }
    lines
}

#[test]
fn a_long_word_breaks_at_a_pattern_point() {
    let layout = build(word(), true);
    let hyphenated: Vec<_> = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .flat_map(|block| block.lines.iter())
        .filter(|placed| placed.line.hyphenated)
        .collect();

    assert!(!hyphenated.is_empty(), "some line is hyphenated");
    // The pieces rejoin into the word, in order.
    let joined: String = lines_text(&layout).join("");
    assert!(joined.contains("前文"), "{joined}");
    assert!(joined.contains("hyphenation"), "{joined}");
    assert!(joined.contains("后文"), "{joined}");
}

#[test]
fn the_hyphen_is_charged_to_the_line() {
    let layout = build(word(), true);
    for page in &layout.pages {
        for block in &page.blocks {
            for placed in &block.lines {
                let line = &placed.line;
                if !line.hyphenated {
                    continue;
                }
                // The hyphen width is inside the measured width, so a justified
                // line still ends at the measure.
                assert!(
                    line.natural_width.0 <= line.target_width.0,
                    "hyphenated line fits: {} > {}",
                    line.natural_width.0,
                    line.target_width.0
                );
            }
        }
    }
}

#[test]
fn without_patterns_the_word_stays_whole() {
    let layout = build(word(), false);
    let hyphenated = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .flat_map(|block| block.lines.iter())
        .any(|placed| placed.line.hyphenated);

    assert!(!hyphenated, "no patterns, no hyphen breaks");
    let joined = lines_text(&layout).join("");
    assert!(joined.contains("hyphenation"), "{joined}");
}

#[test]
fn a_url_is_left_alone() {
    let inlines = vec![text("参见 https://example.com/hyphenation 一节")];
    let layout = build(inlines, true);
    let joined = lines_text(&layout).join("");
    let hyphenated = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .flat_map(|block| block.lines.iter())
        .any(|placed| placed.line.hyphenated);

    assert!(!hyphenated, "a path is not a word: {joined}");
    assert!(
        joined.contains("https://example.com/hyphenation"),
        "{joined}"
    );
}

#[test]
fn canonical_bytes_mark_the_break() {
    let layout = build(word(), true);
    assert!(
        layout.canonical_json().contains("\"hyphenated\":true"),
        "the hash covers where a line broke"
    );
}
