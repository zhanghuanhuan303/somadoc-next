//! Shared fixtures for the A1 test suites.
//!
//! `maximal_document` contains every block kind, every inline kind, table parts,
//! a footnote, generated slots, and raw payloads. If a traversal, validator, or
//! canonical writer misses a node, the tests that renumber and validate this
//! document fail first.

#![allow(dead_code)]

use soma_doc::block::{
    BlockQuote, CodeBlock, Column, Figure, GeneratedBlock, GeneratedKind, Heading, List, ListItem,
    MathBlock, PageBreak, Paragraph, RawBlock, Row, Section, Table, ThematicBreak,
};
use soma_doc::inline::{
    Container, FootnoteRef, Image, LineBreak, LineBreakKind, Link, MathInline, RawInline, Ruby,
};
use soma_doc::meta::{Author, CoverData, Date, DocumentMeta, ImageRef, MetaField, TemplateRef};
use soma_doc::node::NodeMeta;
use soma_doc::raw::{RawCapability, RawContent};
use soma_doc::{Attrs, Block, Cell, CellAlign, Code, Document, FootnoteDef, Inline, Text};
use soma_doc::{SourceId, SourceInfo, SourceKind, SourceMap, SourceSpan};

/// A byte span in source 1.
pub fn span(start: u32, end: u32) -> SourceSpan {
    SourceSpan::at(SourceId(1), start, end)
}

pub fn meta(start: u32, end: u32) -> NodeMeta {
    NodeMeta::at(span(start, end))
}

pub fn text(value: &str) -> Inline {
    Text::new(meta(0, 1), value).into()
}

/// Convenience: validated attributes with a class.
pub fn classed(class: &str) -> Attrs {
    Attrs::new().with_class(class)
}

fn source_map() -> SourceMap {
    let mut map = SourceMap::new();
    map.push(SourceInfo::new(
        SourceId(1),
        "chapter.md",
        SourceKind::Markdown,
    ));
    map
}

fn rich_meta() -> DocumentMeta {
    let mut cover = CoverData::new();
    cover.title = Some("中文排版".into());
    cover.subtitle = Some("一个可验证的排版核心".into());
    cover.authors.push("张三".into());
    cover.date = Some("2026-11".into());
    cover.logo = Some(ImageRef::new("logo.png").with_intrinsic(240, 120));
    cover.fields.push(MetaField::new("edition", "1"));

    DocumentMeta::new()
        .with_title(vec![text("中文排版")])
        .with_author(Author::new("张三").with_affiliation("某出版社"))
        .with_language("zh-Hans")
        .with_date(Date::new(2026, 11, 20))
        .with_keyword("排版")
        .with_keyword("中文")
        .with_field("edition", "1")
        .with_template(TemplateRef::new("book").with_version("1"))
        .with_cover(cover)
}

/// One paragraph that exercises every inline kind.
fn maximal_inline_paragraph() -> Block {
    let raw = RawContent::new("html", "<span class=\"x\">旧</span>")
        .with_fallback("旧")
        .with_capability(RawCapability::Degraded);

    let inlines = vec![
        text("正文"),
        Inline::Code(Code::new(meta(0, 2), "let x = 1;")),
        Inline::Strong(Container::new(
            meta(0, 2),
            vec![Inline::Emphasis(Container::new(
                meta(0, 1),
                vec![text("强调")],
            ))],
        )),
        Inline::Underline(Container::new(meta(0, 1), vec![text("下划线")])),
        Inline::Strikethrough(Container::new(meta(0, 1), vec![text("删除")])),
        Inline::SmallCaps(Container::new(meta(0, 1), vec![text("Small")])),
        Inline::Superscript(Container::new(meta(0, 1), vec![text("2")])),
        Inline::Subscript(Container::new(meta(0, 1), vec![text("n")])),
        Inline::Link(Link::new(
            meta(0, 1),
            "https://example.com",
            vec![text("链接")],
        )),
        Inline::Image(Image::new(
            meta(0, 1),
            ImageRef::new("figure.png")
                .with_alt("图")
                .with_intrinsic(640, 480),
        )),
        Inline::Math(MathInline::new(meta(0, 3), "a^2+b^2=c^2")),
        Inline::FootnoteRef(FootnoteRef::new(meta(0, 2), "1").with_label("1")),
        Inline::Ruby(Ruby::new(meta(0, 2), "漢", "かん")),
        Inline::LineBreak(LineBreak::new(meta(0, 1), LineBreakKind::Hard)),
        Inline::Raw(RawInline::new(meta(0, 1), raw)),
    ];

    Block::Paragraph(Paragraph::new(meta(0, 40), inlines))
}

fn table() -> Block {
    let columns = vec![
        Column::new(meta(0, 1), CellAlign::Left),
        Column::new(meta(0, 1), CellAlign::Right),
    ];
    let header = Row::new(
        meta(0, 2),
        true,
        vec![
            Cell::new(
                meta(0, 1),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 1),
                    vec![text("名称")],
                ))],
            ),
            Cell::new(
                meta(0, 1),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 1),
                    vec![text("值")],
                ))],
            ),
        ],
    );
    let body = Row::new(
        meta(0, 2),
        false,
        vec![
            Cell::new(
                meta(0, 1),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 1),
                    vec![text("宽度")],
                ))],
            ),
            Cell::new(
                meta(0, 1),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 1),
                    vec![text("10.5pt")],
                ))],
            ),
        ],
    );

    let mut table = Table::new(meta(0, 20), columns, vec![header, body]);
    table.caption.push(text("表 1：参数"));
    table.label = Some("tbl:params".into());
    Block::Table(table)
}

/// A document containing every node kind.
pub fn maximal_document() -> Document {
    let mut document = Document::new(rich_meta());
    document.source_map = source_map();

    let mut section = Section::new(meta(0, 8), 1);
    section.page_template = Some("chapter".into());
    section.title = Some(vec![text("第一章")]);
    document.push_block(Block::Section(section));

    document.push_block(Block::Heading(Heading::new(
        meta(0, 12),
        2,
        vec![
            text("小节"),
            Inline::FootnoteRef(FootnoteRef::new(meta(0, 2), "1")),
        ],
    )));
    document.push_block(maximal_inline_paragraph());

    let mut list = List::new(
        meta(0, 30),
        true,
        3,
        vec![
            ListItem::new(
                meta(0, 10),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 5),
                    vec![text("第一项")],
                ))],
            ),
            ListItem::new(
                meta(0, 10),
                vec![Block::Paragraph(Paragraph::new(
                    meta(0, 5),
                    vec![text("第二项")],
                ))],
            ),
        ],
    );
    list.tight = false;
    document.push_block(Block::List(list));

    document.push_block(Block::BlockQuote(BlockQuote::new(
        meta(0, 12),
        vec![Block::Paragraph(Paragraph::new(
            meta(0, 6),
            vec![text("引用")],
        ))],
    )));

    document.push_block(Block::CodeBlock(CodeBlock::new(
        meta(0, 20),
        Some("rust".into()),
        "let x = 1;\n",
    )));
    document.push_block(Block::MathBlock(MathBlock::new(meta(0, 12), "\\sum_i a_i")));

    let mut figure = Figure::new(
        meta(0, 16),
        Image::new(
            meta(0, 8),
            ImageRef::new("figure.png")
                .with_alt("架构图")
                .with_intrinsic(800, 600),
        ),
    );
    figure.caption.push(text("图 1：架构"));
    figure.label = Some("fig:arch".into());
    document.push_block(Block::Figure(figure));

    document.push_block(table());
    document.push_block(Block::ThematicBreak(ThematicBreak::new(meta(0, 3))));
    document.push_block(Block::PageBreak(PageBreak::new(meta(0, 3))));

    let mut toc = GeneratedBlock::new(meta(0, 3), GeneratedKind::Toc);
    toc.blocks.push(Block::Paragraph(Paragraph::new(
        meta(0, 1),
        vec![text("目录")],
    )));
    document.push_block(Block::Generated(toc));

    let mut custom = GeneratedBlock::new(meta(0, 3), GeneratedKind::Custom);
    custom.slot = Some("colophon".into());
    document.push_block(Block::Generated(custom));

    document.push_block(Block::Raw(RawBlock::new(
        meta(0, 12),
        RawContent::new("latex", "\\vspace{1em}")
            .with_fallback("[spacing]")
            .with_capability(RawCapability::Preserved),
    )));

    document.push_footnote(FootnoteDef::new(
        meta(0, 10),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            meta(0, 6),
            vec![text("脚注内容")],
        ))],
    ));

    document.renumber();
    document
}

/// A small document used for byte-level golden tests.
pub fn small_document() -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.source_map = source_map();
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::at(span(0, 9)),
        vec![
            Text::new(NodeMeta::at(span(0, 6)), "你好").into(),
            Inline::Strong(Container::new(
                NodeMeta::at(span(6, 9)),
                vec![Text::new(NodeMeta::at(span(6, 9)), "排版").into()],
            )),
        ],
    )));
    document.renumber();
    document
}
