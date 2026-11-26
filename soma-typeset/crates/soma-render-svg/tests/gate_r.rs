//! Gate R: the SVG renderer must paint exactly the layout coordinates, and the
//! HTML renderer must cover the same atoms. Gate V: the SVG bytes are a visual
//! regression lock.

use soma_core::{BreakConfig, Width};
use soma_layout::{layout, PagedLayout, PlacedBlock, StyledMetrics, TextSource};
use soma_md::parse_markdown;
use soma_metrics::MetricsTable;
use soma_render_svg::render;
use soma_template::{parse_template, resolve, StyledDoc};

const TEMPLATE: &str = r#"
name: svg-fixture
page:
  size: A5
  margin: 14mm
  header: 6mm
  footer: 8mm
  folio: "第 {page} 页 / 共 {pages} 页"
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
      font-size: 15pt
      space-before: 1em
      keep-with-next: true
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

const MARKDOWN: &str = r##"# 标题

正文 **强调**，脚注[^1]。中文排版需要断行与对齐，标点不能出现在行首。

第二段用来制造更多行，看看页面如何填充。Mixed English 与数字 123 也要能排。

[^1]: 脚注内容。
"##;

struct Fixture {
    layout: PagedLayout,
    styled: StyledDoc,
}

fn fixture() -> Fixture {
    let doc = parse_markdown(MARKDOWN);
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&doc, &template).expect("styles");
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let layout = layout(&doc, &styled, &metrics, &BreakConfig::default()).expect("layout");
    Fixture { layout, styled }
}

fn attr<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn is_mark(layout: &PagedLayout, node: u32, atom: u32) -> bool {
    layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.node.0 == node)
        .flat_map(|block| block.marks.iter())
        .any(|mark| mark.atom == atom)
}

/// Baseline and body font size of a marked atom.
fn mark_frame(fixture: &Fixture, node: u32, atom: u32) -> Option<(i32, i32)> {
    for page in &fixture.layout.pages {
        for block in &page.blocks {
            if block.node.0 != node || !block.marks.iter().any(|mark| mark.atom == atom) {
                continue;
            }
            for placed in &block.lines {
                if atom < placed.line.start || atom >= placed.line.end {
                    continue;
                }
                let baseline = fixture.layout.page.margin_top.0
                    + block.y.0
                    + placed.y.0
                    + placed.line.ascent.0
                    + placed.line.leading.0 / 2;
                let style_id = fixture
                    .layout
                    .text
                    .atom_style(block.node, atom)
                    .unwrap_or(block.style);
                let size = fixture
                    .styled
                    .table()
                    .get(style_id)
                    .or_else(|| fixture.styled.table().get(block.style))?
                    .font_size
                    .0;
                return Some((baseline, size));
            }
        }
    }
    None
}

fn text_elements(svg: &str) -> Vec<(&str, &str, &str, &str)> {
    svg.lines()
        .filter(|line| line.starts_with("<text ") && !line.contains("data-footnote-number"))
        .map(|line| {
            (
                attr(line, "data-node").expect("node"),
                attr(line, "data-atom").expect("atom"),
                attr(line, "x").expect("x"),
                attr(line, "y").expect("y"),
            )
        })
        .collect()
}

fn em(width: Width) -> String {
    soma_render_svg::units(width)
}

fn find_atom(
    layout: &PagedLayout,
    page: &soma_layout::Page,
    node: u32,
    atom: u32,
) -> Option<(i32, i32)> {
    let mut collections: Vec<(&PlacedBlock, i32)> = Vec::new();
    for block in &page.blocks {
        collections.push((block, 0));
    }
    for block in &page.header {
        collections.push((block, 0));
    }
    for block in &page.footer {
        collections.push((block, 0));
    }
    let footnote_top = layout.page.content_height().0 - page.footnote_height.0;
    for block in &page.footnotes {
        collections.push((block, footnote_top.max(0)));
    }

    for (block, extra) in collections {
        if block.node.0 != node {
            continue;
        }
        for placed in &block.lines {
            if let Some(candidate) = placed.line.boxes.iter().find(|item| item.atom == atom) {
                return Some((
                    layout.page.margin_left.0 + block.x.0 + candidate.x.0,
                    layout.page.margin_top.0
                        + block.y.0
                        + extra
                        + placed.y.0
                        + placed.line.ascent.0,
                ));
            }
        }
    }
    None
}

#[test]
fn every_atom_is_painted_exactly_once() {
    let fixture = fixture();
    let svg = render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );
    let painted = text_elements(&svg).len();

    let mut expected = 0;
    for page in &fixture.layout.pages {
        for block in page
            .blocks
            .iter()
            .chain(page.header.iter())
            .chain(page.footer.iter())
            .chain(page.footnotes.iter())
        {
            for placed in &block.lines {
                for atom in &placed.line.boxes {
                    if !fixture
                        .layout
                        .text
                        .atom_text(block.node, atom.atom)
                        .is_empty()
                    {
                        expected += 1;
                    }
                }
            }
        }
    }
    assert_eq!(painted, expected, "one <text> per visible atom");
    assert!(expected > 20, "the fixture must be substantial");
}

#[test]
fn svg_coordinates_equal_the_layout_coordinates() {
    let fixture = fixture();
    let svg = render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );

    let mut checked = 0;
    for line in svg
        .lines()
        .filter(|line| line.starts_with("<text ") && !line.contains("data-footnote-number"))
    {
        let node: u32 = attr(line, "data-node").unwrap().parse().unwrap();
        let atom: u32 = attr(line, "data-atom").unwrap().parse().unwrap();
        if is_mark(&fixture.layout, node, atom) {
            continue;
        }
        let x = attr(line, "x").unwrap();
        let y = attr(line, "y").unwrap();

        let mut found = None;
        for page in &fixture.layout.pages {
            if let Some(coords) = find_atom(&fixture.layout, page, node, atom) {
                found = Some(coords);
                break;
            }
        }
        let (expected_x, expected_y) = found.expect("atom in the layout");
        assert_eq!(x, em(Width(expected_x)), "x for node {node} atom {atom}");
        assert_eq!(y, em(Width(expected_y)), "y for node {node} atom {atom}");
        checked += 1;
    }
    assert!(checked > 20);
}

#[test]
fn footnote_marks_are_smaller_and_raised() {
    let fixture = fixture();
    let svg = render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );

    let mut checked = 0;
    for line in svg
        .lines()
        .filter(|line| line.starts_with("<text ") && !line.contains("data-footnote-number"))
    {
        let node: u32 = attr(line, "data-node").unwrap().parse().unwrap();
        let atom: u32 = attr(line, "data-atom").unwrap().parse().unwrap();
        let Some((baseline, size)) = mark_frame(&fixture, node, atom) else {
            continue;
        };
        // ADR-0096: two thirds of the body size, raised two fifths of an em.
        assert_eq!(
            attr(line, "font-size").unwrap(),
            em(Width(size * 2 / 3)),
            "mark size for node {node} atom {atom}"
        );
        assert_eq!(
            attr(line, "y").unwrap(),
            em(Width(baseline - size * 2 / 5)),
            "mark baseline for node {node} atom {atom}"
        );
        checked += 1;
    }
    assert!(checked > 0, "the fixture must contain a footnote mark");
}

#[test]
fn html_and_svg_cover_the_same_atoms() {
    let fixture = fixture();
    let svg = render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );
    let html = soma_render_html::render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );

    let mut svg_atoms: Vec<(String, String)> = text_elements(&svg)
        .into_iter()
        .map(|(node, atom, _, _)| (String::from(node), String::from(atom)))
        .collect();
    let mut html_atoms: Vec<(String, String)> = html
        .lines()
        .filter(|line| line.contains("class=\"atom\""))
        .map(|line| {
            (
                String::from(attr(line, "data-node").expect("node")),
                String::from(attr(line, "data-atom").expect("atom")),
            )
        })
        .collect();
    svg_atoms.sort();
    html_atoms.sort();
    assert_eq!(
        svg_atoms, html_atoms,
        "both renderers must paint the same atoms"
    );
}

#[test]
fn page_fragments_are_embeddable_and_keep_the_layout() {
    let fixture = fixture();
    let pages = soma_render_svg::render_fragments(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );
    assert_eq!(pages.len(), fixture.layout.page_count() as usize);

    for (index, page) in pages.iter().enumerate() {
        let number = index + 1;
        assert!(
            page.body.starts_with(&format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"page\" data-page=\"{number}\" width=\"100%\" height=\"100%\""
            )),
            "page {number} root"
        );
        assert!(page.body.contains("viewBox=\"0 0 "));
        assert!(page.body.contains("<g class=\"block\""), "block groups");
        assert!(!page.body.contains("<script"), "no scripts");
        assert!(!page.body.contains("href=\"http"), "no external links");
    }
}

#[test]
fn svg_bytes_are_a_visual_regression_lock() {
    let fixture = fixture();
    let svg = render(
        &fixture.layout,
        fixture.styled.table(),
        &fixture.layout.text,
    );
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in svg.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert!(svg.lines().count() > 40);
    assert_eq!(hash, GOLDEN_SVG_HASH, "SVG bytes changed; review the diff");
}

/// Updated deliberately when the visual output changes.
/// 2026-11-20: a footnote marker is a small superscript object, not a one-em
/// glyph, and it is drawn as its number.
/// 2026-11-20: a space shrinks by at most a third of its advance (D86), which
/// moves the justified atoms inside their lines by at most 0.006 em. Line
/// breaks are unchanged.
/// 2026-11-20: every coordinate is now a unitless layout unit instead of an
/// `em` length. The old form made each position scale with the surrounding
/// font size, so a page extracted on its own rendered wrong; the root
/// viewport is in pixels at one em = 16 px, and the `viewBox` is unchanged.
/// The layout, and therefore every coordinate's *value*, is identical (D87).
/// 2026-11-21: a footer line sits at the bottom of its band and a header at the
/// top, so the folio no longer touches the text block. Only margin-block
/// position changes; every body coordinate is untouched.
/// 2026-11-25: the footnote area gains a separator rule and a note number,
/// and table cells gain vertical padding (ADR-0082/0083/0084).
/// 2026-11-25: footnote marks are two thirds of the body size and raised two
/// fifths of an em, matching the PDF and LaTeX (ADR-0096).
/// 2026-11-26: each block is a `<g class="block">`, text carries its family
/// and bold/italic attributes so the SVG can be a standalone Studio proof
/// (ADR-0109).
const GOLDEN_SVG_HASH: u64 = 6291207987599973007;
