//! E4: page masters, running headers, folios.

mod common;

use common::{document, engine, metrics, paragraph, styled, text, BOOK, TIGHT};
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, PagedLayout, PlacedBlock, TextSource};
use soma_template::{parse_template, resolve};

fn with_master(source: &str, header: &str, footer: &str, folio: &str) -> String {
    let anchor = if source == TIGHT {
        "  margin: 4mm"
    } else {
        "  margin: 20mm"
    };
    let extra = format!(
        "\n  header: {header}\n  footer: {footer}\n  header-text: running-title\n  folio: \"{folio}\""
    );
    source.replace(anchor, &format!("{anchor}{extra}"))
}

fn block_text(layout: &PagedLayout, block: &PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

fn page_text(layout: &PagedLayout, blocks: &[PlacedBlock]) -> String {
    blocks
        .iter()
        .map(|block| block_text(layout, block))
        .collect()
}

fn book_with_headings() -> soma_doc::Document {
    let mut blocks = Vec::new();
    for chapter in 0..2 {
        blocks.push(Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            1,
            vec![text(&format!("第{}章", chapter + 1))],
        )));
        for _ in 0..10 {
            blocks.push(paragraph(
                "这一段用来把内容推到下一页，中文排版需要断行与对齐。",
            ));
        }
    }
    document(blocks)
}

#[test]
fn header_and_footer_do_not_change_the_content_box() {
    let doc = book_with_headings();
    let plain = styled(&doc, BOOK);
    let master_source = with_master(BOOK, "10mm", "12mm", "第 {page} 页 / 共 {pages} 页");
    let mastered = styled(&doc, &master_source);

    let plain_layout = layout(&doc, &plain, &metrics(), &engine()).expect("layout");
    let master_layout = layout(&doc, &mastered, &metrics(), &engine()).expect("layout");

    assert_eq!(plain_layout.page_count(), master_layout.page_count());
    for (left, right) in plain_layout.pages.iter().zip(master_layout.pages.iter()) {
        assert_eq!(left.blocks, right.blocks, "content must not move");
        assert_eq!(left.footnotes, right.footnotes);
    }
    assert!(master_layout
        .pages
        .iter()
        .all(|page| !page.header.is_empty() && !page.footer.is_empty()));
}

#[test]
fn the_folio_substitutes_page_and_total() {
    let doc = book_with_headings();
    let source = with_master(TIGHT, "3mm", "3mm", "第 {page} 页 / 共 {pages} 页");
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let total = layout.page_count();

    for (index, page) in layout.pages.iter().enumerate() {
        let footer = page_text(&layout, &page.footer);
        assert_eq!(
            footer,
            format!("第{}页/共{}页", index + 1, total),
            "folio text on page {} (spaces are glue, not atoms)",
            index + 1
        );
    }
}

#[test]
fn the_running_header_tracks_the_last_heading() {
    let doc = book_with_headings();
    let source = with_master(TIGHT, "3mm", "3mm", "{page}");
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.page_count() > 2);

    let headers: Vec<String> = layout
        .pages
        .iter()
        .map(|page| page_text(&layout, &page.header))
        .collect();
    assert!(
        headers[0].contains("第1章"),
        "first page header: {:?}",
        headers
    );
    assert!(
        headers.iter().any(|header| header.contains("第2章")),
        "the second chapter must appear in a running header: {:?}",
        headers
    );
}

#[test]
fn margin_bands_sit_outside_the_content_box() {
    let doc = book_with_headings();
    let source = with_master(TIGHT, "3mm", "3mm", "{page}");
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let page = &layout.pages[0];
    let header = page.header.first().expect("header");
    let footer = page.footer.first().expect("footer");
    assert_eq!(header.y.0, -styled.page().header_height.0);
    assert_eq!(footer.y.0, layout.page.content_height().0);
    assert!(header.height.0 <= styled.page().margin_top.0);
    assert!(footer.height.0 <= styled.page().margin_bottom.0);
}

#[test]
fn a_header_taller_than_the_margin_is_rejected() {
    let doc = book_with_headings();
    let source = with_master(TIGHT, "9mm", "3mm", "{page}");
    let template = parse_template(&source).expect("template");
    assert!(resolve(&doc, &template).is_err());
}

#[test]
fn a_template_can_use_a_literal_header() {
    let doc = book_with_headings();
    let source = with_master(TIGHT, "3mm", "3mm", "{page}")
        .replace("header-text: running-title", "header-text: soma-typeset");
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    for page in &layout.pages {
        assert_eq!(page_text(&layout, &page.header), "soma-typeset");
    }
}
