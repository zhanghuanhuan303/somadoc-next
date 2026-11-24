//! Style resolution: cascade + inheritance over DocIR, producing `StyledDoc`.
//!
//! `StyledDoc` is pure data (ADR-0010 §3): a `NodeId -> StyleId` table plus the
//! computed styles. It has no lifetime and can be cached, hashed, or sent to
//! another thread.
//!
//! Cascade rules, in order:
//!
//! 1. a rule matches when every present selector part matches; `within` also
//!    requires a matching block ancestor;
//! 2. matched rules are applied from least to most specific
//!    (`classes`, then `role`/`within`, then `element`), ties by declaration
//!    order;
//! 3. within a rule, properties apply in a fixed order with `font-size` first,
//!    so `line-height` and `em` lengths see the new font size;
//! 4. a later declaration of the same property in the same rule wins.
//!
//! Inherited: `font-size`, `line-height`, `align`, `family`, `color`, `weight`,
//! `slant`, `small-caps`. Everything else starts at its zero/false default on
//! each node.

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::{Align, StyleId, Width};
use soma_doc::{Block, Document, Inline, NodeId};

/// Whether a `font-family` value asks for the sans face.
///
/// `sans`/`mono`/`hei`/`gothic` select the second family; everything else,
/// including an empty value, selects the first. The layout and the PDF backend
/// must agree on this, so it lives next to [`ComputedStyle`] rather than in
/// each of them.
pub fn family_is_sans(family: Option<&str>) -> bool {
    let family = family.unwrap_or("");
    ["sans", "mono", "hei", "gothic"]
        .iter()
        .any(|needle| contains_ignore_ascii_case(family, needle))
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

use crate::template::{FontSlant, FontWeight, Property, PropertyValue, StyleRule, Template};
use crate::units::Rational;
use crate::TemplateError;

/// One computed style. All lengths are in 1/4096 em.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputedStyle {
    pub font_size: Width,
    /// Absolute leading, resolved from the multiplier or fixed length.
    pub line_height: Width,
    pub first_indent: Width,
    /// Extra advance after every atom but the last (tracking).
    pub tracking: Width,
    pub indent_left: Width,
    pub indent_right: Width,
    pub space_before: Width,
    pub space_after: Width,
    /// Stretchable part of the space before a block (vertical glue).
    pub space_before_plus: Width,
    pub space_before_minus: Width,
    pub space_after_plus: Width,
    pub space_after_minus: Width,
    /// Minimum lines of a paragraph that must stay together at the bottom of a
    /// page (orphans) and at the top of the next page (widows).
    pub widow_lines: u8,
    pub orphan_lines: u8,
    /// Allow closing punctuation to hang into the right margin.
    pub hanging_punctuation: bool,
    pub hanging_cap: Width,
    /// Snap line boxes to a baseline grid. Zero disables the grid.
    pub baseline_grid: Width,
    pub align: Align,
    pub family: Option<String>,
    /// Packed `0xRRGGBB`.
    pub color: Option<u32>,
    pub weight: FontWeight,
    pub slant: FontSlant,
    pub underline: bool,
    pub small_caps: bool,
    pub keep_with_next: bool,
    pub page_break_before: bool,
}

impl ComputedStyle {
    /// Defaults before any rule applies. `1em` is the body font size by
    /// definition.
    pub fn base() -> Self {
        Self {
            font_size: Width::EM,
            line_height: Width::from_em_frac(6, 5),
            first_indent: Width::ZERO,
            tracking: Width::ZERO,
            indent_left: Width::ZERO,
            indent_right: Width::ZERO,
            space_before: Width::ZERO,
            space_after: Width::ZERO,
            space_before_plus: Width::ZERO,
            space_before_minus: Width::ZERO,
            space_after_plus: Width::ZERO,
            space_after_minus: Width::ZERO,
            widow_lines: 2,
            orphan_lines: 2,
            hanging_punctuation: false,
            hanging_cap: Width::from_em_frac(1, 2),
            baseline_grid: Width::ZERO,
            align: Align::Justify,
            family: None,
            color: None,
            weight: FontWeight::Normal,
            slant: FontSlant::Normal,
            underline: false,
            small_caps: false,
            keep_with_next: false,
            page_break_before: false,
        }
    }

    fn inherited_from(parent: &Self) -> Self {
        Self {
            font_size: parent.font_size,
            line_height: parent.line_height,
            align: parent.align,
            family: parent.family.clone(),
            color: parent.color,
            weight: parent.weight,
            slant: parent.slant,
            tracking: parent.tracking,
            small_caps: parent.small_caps,
            widow_lines: parent.widow_lines,
            orphan_lines: parent.orphan_lines,
            hanging_punctuation: parent.hanging_punctuation,
            hanging_cap: parent.hanging_cap,
            baseline_grid: parent.baseline_grid,
            ..Self::base()
        }
    }
}

/// Deduplicated computed styles. Indexed by `StyleId`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleTable {
    styles: Vec<ComputedStyle>,
}

impl StyleTable {
    pub fn styles(&self) -> &[ComputedStyle] {
        &self.styles
    }

    pub fn get(&self, id: StyleId) -> Option<&ComputedStyle> {
        self.styles.get(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    fn intern(&mut self, style: ComputedStyle) -> StyleId {
        if let Some(index) = self.styles.iter().position(|seen| *seen == style) {
            return StyleId(index as u16);
        }
        self.styles.push(style);
        StyleId((self.styles.len() - 1) as u16)
    }
}

/// Page geometry resolved to em units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPage {
    pub width: Width,
    pub height: Width,
    pub margin_top: Width,
    pub margin_bottom: Width,
    pub margin_left: Width,
    pub margin_right: Width,
    /// Header band height, drawn inside the top margin.
    pub header_height: Width,
    /// Footer band height, drawn inside the bottom margin.
    pub footer_height: Width,
}

impl ResolvedPage {
    /// Content box width.
    pub fn content_width(self) -> Width {
        Width(self.width.0 - self.margin_left.0 - self.margin_right.0)
    }

    /// Content box height.
    pub fn content_height(self) -> Width {
        Width(self.height.0 - self.margin_top.0 - self.margin_bottom.0)
    }
}

/// Table rule thicknesses resolved to 1/4096 em.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableRules {
    /// Rule between two body rows.
    pub rule: Width,
    /// Rule below the last header row.
    pub header_rule: Width,
    /// Rule above row 0 and below the last row.
    pub outer_rule: Width,
    /// Vertical rule between two body columns.
    pub column_rule: Width,
    /// Vertical rule left of column 0 and right of the last column.
    pub outer_column_rule: Width,
    /// Solid or dashed.
    pub style: crate::template::RuleStyle,
    /// Interned style carrying the rule color.
    pub style_id: StyleId,
}

impl TableRules {
    /// No rule is drawn anywhere.
    pub fn is_empty(self) -> bool {
        self.rule.0 == 0
            && self.header_rule.0 == 0
            && self.outer_rule.0 == 0
            && self.column_rule.0 == 0
            && self.outer_column_rule.0 == 0
    }
}

/// One named page master resolved to em units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMaster {
    pub name: String,
    pub header_height: Width,
    pub footer_height: Width,
    pub header_text: Option<String>,
    pub folio: Option<String>,
}

/// Style ids for the four generated cover lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoverStyles {
    pub title: StyleId,
    pub subtitle: StyleId,
    pub author: StyleId,
    pub date: StyleId,
}

/// The resolved style layer for one document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledDoc {
    /// Sorted by `NodeId`; the order is part of the canonical form.
    node_style: Vec<(NodeId, StyleId)>,
    table: StyleTable,
    page: ResolvedPage,
    footnote: crate::template::FootnoteNumbering,
    table_rules: TableRules,
    masters: Vec<ResolvedMaster>,
    header_text: Option<String>,
    folio: Option<String>,
    /// Table of contents settings, when the template asks for one.
    toc: Option<crate::template::TocSpec>,
    /// Index settings, when the template asks for an index.
    index: Option<crate::template::IndexSpec>,
    /// Cover settings, when the template asks for a cover page.
    cover: Option<crate::template::CoverSpec>,
    /// Automatic numbering scheme.
    numbering: crate::template::NumberingSpec,
    /// Numbering of the pages before the body.
    front_matter: crate::template::FrontMatterNumbering,
    /// Style for TOC entries (`element: toc`).
    toc_style: StyleId,
    /// Style for the TOC heading (`element: tocTitle`).
    toc_title_style: StyleId,
    /// Style for generated figure captions (`element: figureCaption`).
    figure_caption_style: StyleId,
    /// Style for generated table captions (`element: tableCaption`).
    table_caption_style: StyleId,
    /// Style for generated index entries (`element: indexEntry`).
    index_style: StyleId,
    /// Style for the index heading (`element: indexTitle`).
    index_title_style: StyleId,
    /// Styles for the generated cover lines.
    cover_styles: CoverStyles,
    /// Master index for pages that begin a section.
    start_master: Option<usize>,
    /// Root font size in points. One em of `Width` equals this many points.
    body_pt: Rational,
    template_hash: crate::TemplateHash,
}

impl StyledDoc {
    pub fn style_of(&self, node: NodeId) -> Option<StyleId> {
        self.node_style
            .binary_search_by_key(&node, |(id, _)| *id)
            .ok()
            .map(|index| self.node_style[index].1)
    }

    pub fn style(&self, id: StyleId) -> Option<&ComputedStyle> {
        self.table.get(id)
    }

    pub fn table(&self) -> &StyleTable {
        &self.table
    }

    pub fn node_style(&self) -> &[(NodeId, StyleId)] {
        &self.node_style
    }

    pub fn page(&self) -> ResolvedPage {
        self.page
    }

    pub fn footnote_numbering(&self) -> crate::template::FootnoteNumbering {
        self.footnote
    }

    /// Table rules resolved from the template's `table` section.
    pub fn table_rules(&self) -> TableRules {
        self.table_rules
    }

    /// Master 0 is the default `page:` geometry, then `masters:` in order.
    pub fn masters(&self) -> &[ResolvedMaster] {
        &self.masters
    }

    /// Index of a named master, or `None` when the name is unknown.
    pub fn master_index(&self, name: &str) -> Option<usize> {
        self.masters.iter().position(|master| master.name == name)
    }

    /// Master by index; an out-of-range index falls back to the default.
    pub fn master(&self, index: usize) -> &ResolvedMaster {
        self.masters.get(index).unwrap_or(&self.masters[0])
    }

    /// Literal header text, or `running-title` when the template says so.
    pub fn header_text(&self) -> Option<&str> {
        self.header_text.as_deref()
    }

    /// Folio template with `{page}` and `{pages}` placeholders.
    pub fn folio(&self) -> Option<&str> {
        self.folio.as_deref()
    }

    /// Table of contents settings, when the template asks for one.
    pub fn toc(&self) -> Option<&crate::template::TocSpec> {
        self.toc.as_ref()
    }

    /// Style of one TOC entry.
    pub fn toc_style(&self) -> StyleId {
        self.toc_style
    }

    /// Style of the TOC heading.
    pub fn toc_title_style(&self) -> StyleId {
        self.toc_title_style
    }

    /// Style of generated figure captions.
    pub fn figure_caption_style(&self) -> StyleId {
        self.figure_caption_style
    }

    /// Style of generated table captions.
    pub fn table_caption_style(&self) -> StyleId {
        self.table_caption_style
    }

    /// Style of generated index entries.
    pub fn index_style(&self) -> StyleId {
        self.index_style
    }

    /// Style of the index heading.
    pub fn index_title_style(&self) -> StyleId {
        self.index_title_style
    }

    /// Index settings, when the template asks for an index.
    pub fn index(&self) -> Option<&crate::template::IndexSpec> {
        self.index.as_ref()
    }

    /// Cover settings, when the template asks for a cover page.
    pub fn cover(&self) -> Option<&crate::template::CoverSpec> {
        self.cover.as_ref()
    }

    /// Automatic numbering scheme.
    pub fn numbering(&self) -> &crate::template::NumberingSpec {
        &self.numbering
    }

    /// Numbering of the pages before the body.
    pub fn front_matter(&self) -> crate::template::FrontMatterNumbering {
        self.front_matter
    }

    /// Styles of the generated cover lines.
    pub fn cover_styles(&self) -> CoverStyles {
        self.cover_styles
    }

    /// Master index for the first page of a section.
    pub fn start_master(&self) -> Option<usize> {
        self.start_master
    }

    /// Root font size in points. `Width::EM` corresponds to this size.
    pub fn body_pt(&self) -> Rational {
        self.body_pt
    }

    pub fn template_hash(&self) -> crate::TemplateHash {
        self.template_hash
    }

    /// Hash of the canonical style result (`soma.styles/1`).
    pub fn style_hash(&self) -> crate::StyleHash {
        crate::canonical::style_hash(self)
    }

    pub fn canonical_json(&self) -> String {
        crate::canonical::style_json(self)
    }
}

/// Resolve a document against a template.
pub fn resolve(doc: &Document, template: &Template) -> Result<StyledDoc, TemplateError> {
    let body_pt = template.body_font_pt();
    if body_pt.num() <= 0 {
        return Err(TemplateError::new(
            1,
            1,
            "styles",
            "document font-size must be positive",
        ));
    }

    let mut resolver = Resolver {
        rules: &template.styles,
        body_pt,
        table: StyleTable::default(),
        node_style: Vec::new(),
    };

    let root = resolver.compute("document", None, &[], None, ComputedStyle::base(), &[])?;
    resolver.table.intern(root.clone());

    let mut ancestors: Vec<&'static str> = Vec::new();
    if let Some(title) = &doc.meta.title {
        resolver.inlines(title, &root, &mut ancestors)?;
    }
    resolver.blocks(&doc.blocks, &root, &mut ancestors)?;
    for definition in &doc.footnotes {
        let style = resolver.compute(
            "footnote",
            definition.meta.attrs.role.as_deref(),
            &class_refs(&definition.meta.attrs.classes),
            None,
            ComputedStyle::inherited_from(&root),
            &ancestors,
        )?;
        let id = resolver.table.intern(style.clone());
        resolver.node_style.push((definition.meta.id, id));
        let mut footnote_ancestors = ancestors.clone();
        footnote_ancestors.push("footnote");
        resolver.blocks(&definition.blocks, &style, &mut footnote_ancestors)?;
    }

    resolver.node_style.sort_by_key(|(node, _)| *node);

    let (width, height) = template.page.size.dimensions();
    let page = ResolvedPage {
        width: width.resolve(root.font_size, body_pt),
        height: height.resolve(root.font_size, body_pt),
        margin_top: template.page.margin_top.resolve(root.font_size, body_pt),
        margin_bottom: template.page.margin_bottom.resolve(root.font_size, body_pt),
        margin_left: template.page.margin_left.resolve(root.font_size, body_pt),
        margin_right: template.page.margin_right.resolve(root.font_size, body_pt),
        header_height: template.page.header.resolve(root.font_size, body_pt),
        footer_height: template.page.footer.resolve(root.font_size, body_pt),
    };

    if page.header_height.0 > page.margin_top.0 {
        return Err(TemplateError::new(
            1,
            1,
            "page",
            "header height must fit inside the top margin",
        ));
    }
    if page.footer_height.0 > page.margin_bottom.0 {
        return Err(TemplateError::new(
            1,
            1,
            "page",
            "footer height must fit inside the bottom margin",
        ));
    }

    let mut masters = Vec::with_capacity(template.masters.len() + 1);
    masters.push(ResolvedMaster {
        name: String::from("default"),
        header_height: page.header_height,
        footer_height: page.footer_height,
        header_text: template.page.header_text.clone(),
        folio: template.page.folio.clone(),
    });
    for master in &template.masters {
        let header_height = master.header.resolve(root.font_size, body_pt);
        let footer_height = master.footer.resolve(root.font_size, body_pt);
        if header_height.0 > page.margin_top.0 {
            return Err(TemplateError::new(
                1,
                1,
                "masters",
                "master header height must fit inside the top margin",
            ));
        }
        if footer_height.0 > page.margin_bottom.0 {
            return Err(TemplateError::new(
                1,
                1,
                "masters",
                "master footer height must fit inside the bottom margin",
            ));
        }
        masters.push(ResolvedMaster {
            name: master.name.clone(),
            header_height,
            footer_height,
            header_text: master.header_text.clone(),
            folio: master.folio.clone(),
        });
    }

    // Generated content needs styles that no DocIR node carries: TOC entries
    // and the TOC heading. They inherit the document style.
    let toc_style = resolver.compute(
        "toc",
        None,
        &[],
        None,
        ComputedStyle::inherited_from(&root),
        &[],
    )?;
    let toc_style_id = resolver.table.intern(toc_style);
    let cover_styles = CoverStyles {
        title: intern_element(&mut resolver, "coverTitle", &root)?,
        subtitle: intern_element(&mut resolver, "coverSubtitle", &root)?,
        author: intern_element(&mut resolver, "coverAuthor", &root)?,
        date: intern_element(&mut resolver, "coverDate", &root)?,
    };
    let toc_title_style = resolver.compute(
        "tocTitle",
        None,
        &[],
        None,
        ComputedStyle::inherited_from(&root),
        &[],
    )?;
    let toc_title_style_id = resolver.table.intern(toc_title_style);
    let figure_caption_style = intern_element(&mut resolver, "figureCaption", &root)?;
    let table_caption_style = intern_element(&mut resolver, "tableCaption", &root)?;
    let index_style = intern_element(&mut resolver, "indexEntry", &root)?;
    let index_title_style = intern_element(&mut resolver, "indexTitle", &root)?;

    // Table rules carry their own color; when the template does not set one
    // they inherit the document color (and renderers fall back to black).
    let rule_style = resolver.table.intern(ComputedStyle {
        color: template.table.color.or(root.color),
        ..root.clone()
    });

    // The section-start master names a defined master; `default` means index 0.
    let start_master = match template.page.start_master.as_deref() {
        None => None,
        Some("default") => Some(0),
        Some(name) => match masters.iter().position(|master| master.name == name) {
            Some(index) => Some(index),
            None => {
                return Err(TemplateError::new(
                    1,
                    1,
                    "page.start-master",
                    "start-master must name a defined master or default",
                ))
            }
        },
    };

    Ok(StyledDoc {
        node_style: resolver.node_style,
        table: resolver.table,
        page,
        footnote: template.footnote.numbering,
        toc: template.toc.clone(),
        index: template.index.clone(),
        cover: template.cover.clone(),
        numbering: template.numbering.clone(),
        front_matter: template.page.front_matter,
        toc_style: toc_style_id,
        toc_title_style: toc_title_style_id,
        figure_caption_style,
        table_caption_style,
        index_style,
        index_title_style,
        cover_styles,
        table_rules: TableRules {
            rule: template.table.rule.resolve(root.font_size, body_pt),
            header_rule: template.table.header_rule.resolve(root.font_size, body_pt),
            outer_rule: template.table.outer_rule.resolve(root.font_size, body_pt),
            column_rule: template.table.column_rule.resolve(root.font_size, body_pt),
            outer_column_rule: template
                .table
                .outer_column_rule
                .resolve(root.font_size, body_pt),
            style: template.table.style,
            style_id: rule_style,
        },
        masters,
        header_text: template.page.header_text.clone(),
        folio: template.page.folio.clone(),
        start_master,
        body_pt,
        template_hash: template.template_hash(),
    })
}

/// Heading and section nodes carry a level selectors can match.
fn block_level(block: &Block) -> Option<u8> {
    match block {
        Block::Heading(heading) => Some(heading.level),
        Block::Section(section) => Some(section.level),
        _ => None,
    }
}

/// Resolve and intern one generated element style.
fn intern_element(
    resolver: &mut Resolver<'_>,
    element: &str,
    root: &ComputedStyle,
) -> Result<StyleId, TemplateError> {
    let style = resolver.compute(
        element,
        None,
        &[],
        None,
        ComputedStyle::inherited_from(root),
        &[],
    )?;
    Ok(resolver.table.intern(style))
}

struct Resolver<'a> {
    rules: &'a [StyleRule],
    body_pt: Rational,
    table: StyleTable,
    node_style: Vec<(NodeId, StyleId)>,
}

impl Resolver<'_> {
    fn blocks(
        &mut self,
        blocks: &[Block],
        parent: &ComputedStyle,
        ancestors: &mut Vec<&'static str>,
    ) -> Result<(), TemplateError> {
        for block in blocks {
            let meta = block.meta();
            let element = block_element(block);
            let mut base = ComputedStyle::inherited_from(parent);
            if element == "mathBlock" {
                // TeX's display skips: a template that does not mention
                // `mathBlock` still gets breathing room, so consecutive
                // formulas do not touch (ADR-0080). An explicit rule overrides
                // every value here.
                base.space_before = base.font_size;
                base.space_after = base.font_size;
                base.space_before_plus = Width(base.font_size.0 / 5);
                base.space_after_plus = Width(base.font_size.0 / 5);
                base.space_before_minus = Width(base.font_size.0 / 2);
                base.space_after_minus = Width(base.font_size.0 / 2);
            }
            let style = self.compute(
                element,
                meta.attrs.role.as_deref(),
                &class_refs(&meta.attrs.classes),
                block_level(block),
                base,
                ancestors,
            )?;
            let id = self.table.intern(style.clone());
            self.node_style.push((meta.id, id));
            ancestors.push(element);
            self.visit_block(block, &style, ancestors)?;
            ancestors.pop();
        }
        Ok(())
    }

    fn visit_block(
        &mut self,
        block: &Block,
        style: &ComputedStyle,
        ancestors: &mut Vec<&'static str>,
    ) -> Result<(), TemplateError> {
        match block {
            Block::Paragraph(node) => self.inlines(&node.inlines, style, ancestors),
            Block::Heading(node) => self.inlines(&node.inlines, style, ancestors),
            Block::List(node) => {
                for item in &node.items {
                    let meta = &item.meta;
                    let item_style = self.compute(
                        "listItem",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        None,
                        ComputedStyle::inherited_from(style),
                        ancestors,
                    )?;
                    let id = self.table.intern(item_style.clone());
                    self.node_style.push((meta.id, id));
                    ancestors.push("listItem");
                    self.blocks(&item.blocks, &item_style, ancestors)?;
                    ancestors.pop();
                }
                Ok(())
            }
            Block::BlockQuote(node) => self.blocks(&node.blocks, style, ancestors),
            Block::Figure(node) => {
                let meta = &node.image.meta;
                let image_style = self.compute(
                    "image",
                    meta.attrs.role.as_deref(),
                    &class_refs(&meta.attrs.classes),
                    None,
                    ComputedStyle::inherited_from(style),
                    ancestors,
                )?;
                let id = self.table.intern(image_style.clone());
                self.node_style.push((meta.id, id));
                self.caption(&node.caption, style, "figureCaption", ancestors)
            }
            Block::Table(node) => {
                for column in &node.columns {
                    let meta = &column.meta;
                    let column_style = self.compute(
                        "column",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        None,
                        ComputedStyle::inherited_from(style),
                        ancestors,
                    )?;
                    let id = self.table.intern(column_style);
                    self.node_style.push((meta.id, id));
                }
                self.caption(&node.caption, style, "tableCaption", ancestors)?;
                for row in &node.rows {
                    let meta = &row.meta;
                    let row_style = self.compute(
                        "row",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        None,
                        ComputedStyle::inherited_from(style),
                        ancestors,
                    )?;
                    let row_id = self.table.intern(row_style.clone());
                    self.node_style.push((meta.id, row_id));
                    ancestors.push("row");
                    for cell in &row.cells {
                        let cell_meta = &cell.meta;
                        let cell_style = self.compute(
                            "cell",
                            cell_meta.attrs.role.as_deref(),
                            &class_refs(&cell_meta.attrs.classes),
                            None,
                            ComputedStyle::inherited_from(&row_style),
                            ancestors,
                        )?;
                        let cell_id = self.table.intern(cell_style.clone());
                        self.node_style.push((cell_meta.id, cell_id));
                        ancestors.push("cell");
                        self.blocks(&cell.blocks, &cell_style, ancestors)?;
                        ancestors.pop();
                    }
                    ancestors.pop();
                }
                Ok(())
            }
            Block::Section(node) => {
                if let Some(title) = &node.title {
                    self.inlines(title, style, ancestors)?;
                }
                Ok(())
            }
            Block::Generated(node) => self.blocks(&node.blocks, style, ancestors),
            Block::CodeBlock(_)
            | Block::MathBlock(_)
            | Block::ThematicBreak(_)
            | Block::PageBreak(_)
            | Block::Raw(_) => Ok(()),
        }
    }

    /// Caption inlines resolve under their generated element (ADR-0033), so a
    /// template can style captions without touching the block they belong to.
    fn caption(
        &mut self,
        caption: &[Inline],
        parent: &ComputedStyle,
        element: &'static str,
        ancestors: &mut Vec<&'static str>,
    ) -> Result<(), TemplateError> {
        let style = self.compute(
            element,
            None,
            &[],
            None,
            ComputedStyle::inherited_from(parent),
            ancestors,
        )?;
        ancestors.push(element);
        let result = self.inlines(caption, &style, ancestors);
        ancestors.pop();
        result
    }

    fn inlines(
        &mut self,
        inlines: &[Inline],
        parent: &ComputedStyle,
        ancestors: &mut Vec<&'static str>,
    ) -> Result<(), TemplateError> {
        for inline in inlines {
            let meta = inline.meta();
            let element = inline_element(inline);
            let style = self.compute(
                element,
                meta.attrs.role.as_deref(),
                &class_refs(&meta.attrs.classes),
                None,
                ComputedStyle::inherited_from(parent),
                ancestors,
            )?;
            let id = self.table.intern(style.clone());
            self.node_style.push((meta.id, id));
            ancestors.push(element);
            self.inlines(inline.children(), &style, ancestors)?;
            ancestors.pop();
        }
        Ok(())
    }

    fn compute(
        &mut self,
        element: &str,
        role: Option<&str>,
        classes: &[&str],
        level: Option<u8>,
        mut style: ComputedStyle,
        ancestors: &[&str],
    ) -> Result<ComputedStyle, TemplateError> {
        let mut matched: Vec<(u32, (u32, u32, u32), &StyleRule)> = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                rule.selector
                    .matches(element, role, classes, ancestors, level)
            })
            .map(|(index, rule)| (index as u32, rule.selector.specificity(), rule))
            .collect();
        matched.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));

        for (_, _, rule) in matched {
            for property in PROPERTY_ORDER {
                if let Some(declaration) = rule
                    .declarations
                    .iter()
                    .rev()
                    .find(|declaration| declaration.property == property)
                {
                    apply(&mut style, property, &declaration.value, self.body_pt);
                }
            }
        }

        // All four alignments are supported. The kernel only justifies or
        // flushes left; `soma-layout` shifts centered and right-aligned lines
        // after breaking, so the frozen DP never sees them.
        Ok(style)
    }
}

/// Fixed application order: `font-size` first, then `line-height`, then the
/// remaining properties. This makes the result independent of YAML key order.
const PROPERTY_ORDER: [Property; 26] = [
    Property::FontSize,
    Property::LineHeight,
    Property::FirstIndent,
    Property::LetterSpacing,
    Property::IndentLeft,
    Property::IndentRight,
    Property::SpaceBefore,
    Property::SpaceAfter,
    Property::SpaceBeforePlus,
    Property::SpaceBeforeMinus,
    Property::SpaceAfterPlus,
    Property::SpaceAfterMinus,
    Property::WidowLines,
    Property::OrphanLines,
    Property::BaselineGrid,
    Property::HangingCap,
    Property::HangingPunctuation,
    Property::Align,
    Property::Family,
    Property::Color,
    Property::Weight,
    Property::Slant,
    Property::Underline,
    Property::SmallCaps,
    Property::KeepWithNext,
    Property::PageBreakBefore,
];

fn apply(style: &mut ComputedStyle, property: Property, value: &PropertyValue, body_pt: Rational) {
    let font_size = style.font_size;
    match (property, value) {
        (Property::FontSize, PropertyValue::Length(length)) => {
            style.font_size = length.resolve(font_size, body_pt);
        }
        (Property::FirstIndent, PropertyValue::Length(length)) => {
            style.first_indent = length.resolve(font_size, body_pt);
        }
        (Property::LetterSpacing, PropertyValue::Length(length)) => {
            style.tracking = length.resolve(font_size, body_pt);
        }
        (Property::IndentLeft, PropertyValue::Length(length)) => {
            style.indent_left = length.resolve(font_size, body_pt);
        }
        (Property::IndentRight, PropertyValue::Length(length)) => {
            style.indent_right = length.resolve(font_size, body_pt);
        }
        (Property::SpaceBefore, PropertyValue::Length(length)) => {
            style.space_before = length.resolve(font_size, body_pt);
        }
        (Property::SpaceAfter, PropertyValue::Length(length)) => {
            style.space_after = length.resolve(font_size, body_pt);
        }
        (Property::SpaceBeforePlus, PropertyValue::Length(length)) => {
            style.space_before_plus = length.resolve(font_size, body_pt);
        }
        (Property::SpaceBeforeMinus, PropertyValue::Length(length)) => {
            style.space_before_minus = length.resolve(font_size, body_pt);
        }
        (Property::SpaceAfterPlus, PropertyValue::Length(length)) => {
            style.space_after_plus = length.resolve(font_size, body_pt);
        }
        (Property::SpaceAfterMinus, PropertyValue::Length(length)) => {
            style.space_after_minus = length.resolve(font_size, body_pt);
        }
        (Property::WidowLines, PropertyValue::Count(count)) => style.widow_lines = *count,
        (Property::OrphanLines, PropertyValue::Count(count)) => style.orphan_lines = *count,
        (Property::HangingPunctuation, PropertyValue::Bool(flag)) => {
            style.hanging_punctuation = *flag;
        }
        (Property::HangingCap, PropertyValue::Length(length)) => {
            style.hanging_cap = length.resolve(font_size, body_pt);
        }
        (Property::BaselineGrid, PropertyValue::Length(length)) => {
            style.baseline_grid = length.resolve(font_size, body_pt);
        }
        (Property::LineHeight, PropertyValue::LineHeight(spec)) => {
            style.line_height = spec.resolve(style.font_size, body_pt);
        }
        (Property::Align, PropertyValue::Align(align)) => style.align = *align,
        (Property::Family, PropertyValue::Text(text)) => style.family = Some(text.clone()),
        (Property::Color, PropertyValue::Color(color)) => style.color = Some(*color),
        (Property::Weight, PropertyValue::Weight(weight)) => style.weight = *weight,
        (Property::Slant, PropertyValue::Slant(slant)) => style.slant = *slant,
        (Property::Underline, PropertyValue::Bool(flag)) => style.underline = *flag,
        (Property::SmallCaps, PropertyValue::Bool(flag)) => style.small_caps = *flag,
        (Property::KeepWithNext, PropertyValue::Bool(flag)) => style.keep_with_next = *flag,
        (Property::PageBreakBefore, PropertyValue::Bool(flag)) => style.page_break_before = *flag,
        _ => {}
    }
}

fn class_refs(classes: &[String]) -> Vec<&str> {
    classes.iter().map(String::as_str).collect()
}

fn block_element(block: &Block) -> &'static str {
    match block {
        Block::Paragraph(_) => "paragraph",
        Block::Heading(_) => "heading",
        Block::List(_) => "list",
        Block::BlockQuote(_) => "blockQuote",
        Block::CodeBlock(_) => "codeBlock",
        Block::MathBlock(_) => "mathBlock",
        Block::Figure(_) => "figure",
        Block::Table(_) => "table",
        Block::ThematicBreak(_) => "thematicBreak",
        Block::PageBreak(_) => "pageBreak",
        Block::Section(_) => "section",
        Block::Generated(_) => "generated",
        Block::Raw(_) => "rawBlock",
    }
}

fn inline_element(inline: &Inline) -> &'static str {
    match inline {
        Inline::Text(_) => "text",
        Inline::Code(_) => "code",
        Inline::Strong(_) => "strong",
        Inline::Emphasis(_) => "emphasis",
        Inline::Underline(_) => "underline",
        Inline::Strikethrough(_) => "strikethrough",
        Inline::SmallCaps(_) => "smallCaps",
        Inline::Superscript(_) => "superscript",
        Inline::Subscript(_) => "subscript",
        Inline::Link(_) => "link",
        Inline::Image(_) => "image",
        Inline::Math(_) => "math",
        Inline::FootnoteRef(_) => "footnoteRef",
        Inline::Ruby(_) => "ruby",
        Inline::Index(_) => "indexRef",
        Inline::LineBreak(_) => "lineBreak",
        Inline::Raw(_) => "rawInline",
    }
}
