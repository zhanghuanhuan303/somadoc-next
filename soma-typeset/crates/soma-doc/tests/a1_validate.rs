//! A1 validation tests: every structural invariant has a failing case.

mod common;

use common::classed;
use soma_doc::block::{
    Cell, Column, GeneratedBlock, GeneratedKind, Heading, List, ListItem, Paragraph, Row, Section,
    Table,
};
use soma_doc::inline::{Container, FootnoteRef, Image, MathInline, RawInline};
use soma_doc::meta::{Date, DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::raw::RawContent;
use soma_doc::{Attrs, Block, CellAlign, Code, Document, FootnoteDef, Inline, Text};
use soma_doc::{SourceId, SourceInfo, SourceKind, SourceSpan};

fn paragraph_doc(inlines: Vec<Inline>) -> Document {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
    )));
    document.renumber();
    document
}

fn assert_invalid(document: &Document, needle: &str) {
    let error = document.validate().expect_err("document must be invalid");
    assert!(
        error.reason.contains(needle) || error.path.contains(needle),
        "unexpected error: {error}"
    );
}

#[test]
fn unassigned_node_ids_are_rejected() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));

    assert_invalid(&document, "node id must be assigned");
}

#[test]
fn duplicate_node_ids_are_rejected() {
    let mut document = paragraph_doc(vec![Text::new(NodeMeta::synthesized(), "a").into()]);
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), "b").into()],
    )));
    document.renumber();

    let reused = document.blocks[0].meta().id;
    document.blocks[1].meta_mut().id = reused;

    assert_invalid(&document, "node id is duplicated");
}

#[test]
fn empty_text_nodes_are_rejected() {
    let document = paragraph_doc(vec![Text::new(NodeMeta::synthesized(), "").into()]);
    assert_invalid(&document, "value must not be empty");
}

#[test]
fn heading_levels_are_bounded() {
    for level in [0u8, 7] {
        let mut document = Document::new(DocumentMeta::new());
        document.push_block(Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            level,
            vec![Text::new(NodeMeta::synthesized(), "t").into()],
        )));
        document.renumber();
        assert_invalid(&document, "heading level");
    }

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        6,
        vec![Text::new(NodeMeta::synthesized(), "t").into()],
    )));
    document.renumber();
    assert!(document.validate().is_ok());
}

#[test]
fn section_levels_are_bounded() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Section(Section::new(NodeMeta::synthesized(), 9)));
    document.renumber();
    assert_invalid(&document, "section level");
}

#[test]
fn footnote_references_must_resolve() {
    let document = paragraph_doc(vec![Inline::FootnoteRef(FootnoteRef::new(
        NodeMeta::synthesized(),
        "missing",
    ))]);
    assert_invalid(&document, "no matching definition");
}

#[test]
fn footnote_labels_must_be_unique() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), "a").into()],
        ))],
    ));
    document.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), "b").into()],
        ))],
    ));
    document.renumber();

    assert_invalid(&document, "footnote label is duplicated");
}

#[test]
fn table_rows_must_match_the_column_count() {
    let columns = vec![
        Column::new(NodeMeta::synthesized(), CellAlign::Left),
        Column::new(NodeMeta::synthesized(), CellAlign::Default),
    ];
    let row = Row::new(
        NodeMeta::synthesized(),
        false,
        vec![Cell::new(NodeMeta::synthesized(), vec![])],
    );
    let table = Table::new(NodeMeta::synthesized(), columns, vec![row]);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert_invalid(&document, "colspan sum");
}

#[test]
fn tables_with_rows_require_columns() {
    let row = Row::new(
        NodeMeta::synthesized(),
        false,
        vec![Cell::new(NodeMeta::synthesized(), vec![])],
    );
    let table = Table::new(NodeMeta::synthesized(), vec![], vec![row]);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert_invalid(&document, "must declare columns");
}

#[test]
fn cell_spans_must_be_positive() {
    let columns = vec![Column::new(NodeMeta::synthesized(), CellAlign::Left)];
    let cell = Cell::new(NodeMeta::synthesized(), vec![]).with_spans(0, 1);
    let row = Row::new(NodeMeta::synthesized(), false, vec![cell]);
    let table = Table::new(NodeMeta::synthesized(), columns, vec![row]);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert_invalid(&document, "colspan must be");
}

#[test]
fn attrs_reject_invalid_and_duplicate_classes() {
    let mut empty = Document::new(DocumentMeta::new());
    empty.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized().with_attrs(classed("")),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));
    empty.renumber();
    assert_invalid(&empty, "class is invalid");

    let mut duplicated = Document::new(DocumentMeta::new());
    duplicated.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized().with_attrs(Attrs::new().with_class("a").with_class("a")),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));
    duplicated.renumber();
    assert_invalid(&duplicated, "class is duplicated");

    let mut too_many = Attrs::new();
    for index in 0..soma_doc::MAX_CLASSES + 1 {
        too_many.classes.push(format!("c{index}"));
    }
    let mut many = Document::new(DocumentMeta::new());
    many.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized().with_attrs(too_many),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));
    many.renumber();
    assert_invalid(&many, "too many classes");
}

#[test]
fn anchors_must_be_unique() {
    let mut document = Document::new(DocumentMeta::new());
    for value in ["a", "b"] {
        document.push_block(Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized().with_anchor("same"),
            vec![Text::new(NodeMeta::synthesized(), value).into()],
        )));
    }
    document.renumber();

    assert_invalid(&document, "anchor is duplicated");
}

#[test]
fn spans_must_be_ordered() {
    let mut document = Document::new(DocumentMeta::new());
    document
        .source_map
        .push(SourceInfo::new(SourceId(1), "a.md", SourceKind::Markdown));
    let bad = SourceSpan::at(SourceId(1), 9, 2);
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::at(bad),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));
    document.renumber();

    assert_invalid(&document, "span start");
}

#[test]
fn spans_must_reference_a_known_source() {
    let mut document = Document::new(DocumentMeta::new());
    let unknown = SourceSpan::at(SourceId(7), 0, 2);
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::at(unknown),
        vec![Text::new(NodeMeta::synthesized(), "x").into()],
    )));
    document.renumber();

    assert_invalid(&document, "unknown source");
}

#[test]
fn source_map_rejects_the_reserved_id() {
    let mut document = Document::new(DocumentMeta::new());
    document.source_map.push(SourceInfo::new(
        SourceId::UNKNOWN,
        "a.md",
        SourceKind::Markdown,
    ));

    assert_invalid(&document, "reserved");
}

#[test]
fn nesting_depth_is_bounded() {
    let mut inline: Inline = Text::new(NodeMeta::synthesized(), "deep").into();
    for _ in 0..soma_doc::MAX_DEPTH + 4 {
        inline = Inline::Strong(Container::new(NodeMeta::synthesized(), vec![inline]));
    }

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![inline],
    )));
    document.renumber();

    assert_invalid(&document, "nesting depth");
}

#[test]
fn list_start_rules() {
    let item = ListItem::new(
        NodeMeta::synthesized(),
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), "x").into()],
        ))],
    );

    let mut unordered = Document::new(DocumentMeta::new());
    unordered.push_block(Block::List(List::new(
        NodeMeta::synthesized(),
        false,
        2,
        vec![item.clone()],
    )));
    unordered.renumber();
    assert_invalid(&unordered, "unordered list start");

    let mut ordered = Document::new(DocumentMeta::new());
    ordered.push_block(Block::List(List::new(
        NodeMeta::synthesized(),
        true,
        0,
        vec![item],
    )));
    ordered.renumber();
    assert_invalid(&ordered, "ordered list start");
}

#[test]
fn custom_generated_blocks_require_a_slot() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Generated(GeneratedBlock::new(
        NodeMeta::synthesized(),
        GeneratedKind::Custom,
    )));
    document.renumber();

    assert_invalid(&document, "requires a slot");
}

#[test]
fn metadata_is_checked() {
    let mut bad_date = Document::new(DocumentMeta::new().with_date(Date::new(2026, 13, 1)));
    bad_date.renumber();
    assert_invalid(&bad_date, "date is out of range");

    let mut duplicate_keywords = Document::new(
        DocumentMeta::new()
            .with_keyword("排版")
            .with_keyword("排版"),
    );
    duplicate_keywords.renumber();
    assert_invalid(&duplicate_keywords, "keyword is duplicated");

    let mut duplicate_fields = Document::new(
        DocumentMeta::new()
            .with_field("k", "1")
            .with_field("k", "2"),
    );
    duplicate_fields.renumber();
    assert_invalid(&duplicate_fields, "field key is duplicated");
}

#[test]
fn images_are_checked() {
    let mut empty_src = Document::new(DocumentMeta::new());
    empty_src.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Image(Image::new(
            NodeMeta::synthesized(),
            ImageRef::new(""),
        ))],
    )));
    empty_src.renumber();
    assert_invalid(&empty_src, "src");

    let mut bad_intrinsic = Document::new(DocumentMeta::new());
    bad_intrinsic.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Image(Image::new(
            NodeMeta::synthesized(),
            ImageRef::new("a.png").with_intrinsic(0, 10),
        ))],
    )));
    bad_intrinsic.renumber();
    assert_invalid(&bad_intrinsic, "intrinsic size");
}

#[test]
fn raw_and_math_payloads_are_checked() {
    let mut empty_math = Document::new(DocumentMeta::new());
    empty_math.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Math(MathInline::new(NodeMeta::synthesized(), ""))],
    )));
    empty_math.renumber();
    assert_invalid(&empty_math, "math source");

    let mut bad_raw = Document::new(DocumentMeta::new());
    bad_raw.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Raw(RawInline::new(
            NodeMeta::synthesized(),
            RawContent::new("", "x"),
        ))],
    )));
    bad_raw.renumber();
    assert_invalid(&bad_raw, "raw format");
}

#[test]
fn code_language_tag_must_be_a_token() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::CodeBlock(soma_doc::block::CodeBlock::new(
        NodeMeta::synthesized(),
        Some("rust lang".into()),
        "x",
    )));
    document.renumber();

    assert_invalid(&document, "language tag");
}

#[test]
fn valid_minimal_document_passes() {
    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized().with_attrs(classed("body")),
        vec![
            Text::new(NodeMeta::synthesized(), "中文").into(),
            Code::new(NodeMeta::synthesized(), "code").into(),
        ],
    )));
    document.renumber();

    assert!(document.validate().is_ok());
}

#[test]
fn rowspans_must_fit_the_table() {
    let columns = vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)];
    let tall = Cell::new(NodeMeta::synthesized(), vec![]).with_spans(1, 3);
    let rows = vec![
        Row::new(NodeMeta::synthesized(), false, vec![tall]),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![Cell::new(NodeMeta::synthesized(), vec![])],
        ),
    ];
    let table = Table::new(NodeMeta::synthesized(), columns, rows);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert_invalid(&document, "rowspan exceeds");
}

#[test]
fn overlapping_cells_are_rejected() {
    let columns = (0..3)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect::<Vec<_>>();
    let rows = vec![
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![
                Cell::new(NodeMeta::synthesized(), vec![]).with_spans(1, 2),
                Cell::new(NodeMeta::synthesized(), vec![]),
                Cell::new(NodeMeta::synthesized(), vec![]).with_spans(1, 2),
            ],
        ),
        // Columns 0 and 2 are occupied from above; a colspan-2 cell starting at
        // column 1 would overlap column 2.
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![Cell::new(NodeMeta::synthesized(), vec![]).with_spans(2, 1)],
        ),
    ];
    let table = Table::new(NodeMeta::synthesized(), columns, rows);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert_invalid(&document, "overlaps");
}

#[test]
fn a_rowspan_grid_is_accepted() {
    let columns = vec![
        Column::new(NodeMeta::synthesized(), CellAlign::Default),
        Column::new(NodeMeta::synthesized(), CellAlign::Default),
    ];
    let rows = vec![
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![
                Cell::new(NodeMeta::synthesized(), vec![]).with_spans(1, 2),
                Cell::new(NodeMeta::synthesized(), vec![]),
            ],
        ),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![Cell::new(NodeMeta::synthesized(), vec![])],
        ),
    ];
    let table = Table::new(NodeMeta::synthesized(), columns, rows);

    let mut document = Document::new(DocumentMeta::new());
    document.push_block(Block::Table(table));
    document.renumber();

    assert!(document.validate().is_ok(), "{:?}", document.validate());
}
