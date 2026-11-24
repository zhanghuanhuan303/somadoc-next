//! Link reference definitions resolve collapsed and full references.

use soma_doc::{Block, Inline};

fn paragraph(document: &soma_doc::Document) -> &soma_doc::block::Paragraph {
    match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

#[test]
fn a_full_reference_becomes_a_link_and_the_definition_disappears() {
    let document =
        soma_md::parse_markdown("[指南][docs]\n\n[docs]: https://example.com \"标题\"\n");
    assert_eq!(document.blocks.len(), 1, "the definition line is consumed");
    let paragraph = paragraph(&document);
    let link = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Link(link) => Some(link),
            _ => None,
        })
        .expect("link");
    assert_eq!(link.href, "https://example.com");
    assert_eq!(link.title.as_deref(), Some("标题"));
    assert_eq!(document.plain_text().trim(), "指南");
}

#[test]
fn a_collapsed_reference_uses_the_text_as_the_label() {
    let document = soma_md::parse_markdown("[文档][]\n\n[文档]: <https://example.com/a b>\n");
    let paragraph = paragraph(&document);
    let link = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Link(link) => Some(link),
            _ => None,
        })
        .expect("link");
    assert_eq!(link.href, "https://example.com/a b");
}

#[test]
fn an_image_reference_keeps_its_alt_text() {
    let document = soma_md::parse_markdown("![图][img]\n\n[img]: a.png \"图片\"\n");
    let paragraph = paragraph(&document);
    let image = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Image(image) => Some(image),
            _ => None,
        })
        .expect("image");
    assert_eq!(image.source.src, "a.png");
    assert_eq!(image.source.alt.as_deref(), Some("图"));
    assert_eq!(image.source.title.as_deref(), Some("图片"));
}

#[test]
fn an_undefined_reference_stays_literal_text() {
    let document = soma_md::parse_markdown("[没有][missing]\n");
    assert_eq!(document.plain_text().trim(), "[没有][missing]");
    assert_eq!(document.blocks.len(), 1);
}

#[test]
fn labels_are_case_insensitive_and_whitespace_collapsed() {
    let document = soma_md::parse_markdown("[x][My   Label]\n\n[my label]: https://example.com\n");
    let paragraph = paragraph(&document);
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Link(_))));
}
