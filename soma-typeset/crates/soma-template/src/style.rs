//! Style resolution: cascade + inheritance over DocIR, producing `StyledDoc`.
//!
//! `StyledDoc` is pure data (ADR-0010 §3): a `NodeId -> StyleId` table plus the
//! computed styles. It has no lifetime and can be cached, hashed, or sent to
//! another thread.
//!
//! Cascade rules, in order:
//!
//! 1. a rule matches when every present selector part matches;
//! 2. matched rules are applied from least to most specific
//!    (`classes`, then `role`, then `element`), ties by declaration order;
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
    pub indent_left: Width,
    pub indent_right: Width,
    pub space_before: Width,
    pub space_after: Width,
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
            indent_left: Width::ZERO,
            indent_right: Width::ZERO,
            space_before: Width::ZERO,
            space_after: Width::ZERO,
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
            small_caps: parent.small_caps,
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

/// The resolved style layer for one document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledDoc {
    /// Sorted by `NodeId`; the order is part of the canonical form.
    node_style: Vec<(NodeId, StyleId)>,
    table: StyleTable,
    page: ResolvedPage,
    footnote: crate::template::FootnoteNumbering,
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

    let root = resolver.compute("document", None, &[], ComputedStyle::base())?;
    resolver.table.intern(root.clone());

    if let Some(title) = &doc.meta.title {
        resolver.inlines(title, &root)?;
    }
    resolver.blocks(&doc.blocks, &root)?;
    for definition in &doc.footnotes {
        let style = resolver.compute(
            "footnote",
            definition.meta.attrs.role.as_deref(),
            &class_refs(&definition.meta.attrs.classes),
            ComputedStyle::inherited_from(&root),
        )?;
        let id = resolver.table.intern(style.clone());
        resolver.node_style.push((definition.meta.id, id));
        resolver.blocks(&definition.blocks, &style)?;
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
    };

    Ok(StyledDoc {
        node_style: resolver.node_style,
        table: resolver.table,
        page,
        footnote: template.footnote.numbering,
        template_hash: template.template_hash(),
    })
}

struct Resolver<'a> {
    rules: &'a [StyleRule],
    body_pt: Rational,
    table: StyleTable,
    node_style: Vec<(NodeId, StyleId)>,
}

impl Resolver<'_> {
    fn blocks(&mut self, blocks: &[Block], parent: &ComputedStyle) -> Result<(), TemplateError> {
        for block in blocks {
            let meta = block.meta();
            let style = self.compute(
                block_element(block),
                meta.attrs.role.as_deref(),
                &class_refs(&meta.attrs.classes),
                ComputedStyle::inherited_from(parent),
            )?;
            let id = self.table.intern(style.clone());
            self.node_style.push((meta.id, id));
            self.visit_block(block, &style)?;
        }
        Ok(())
    }

    fn visit_block(&mut self, block: &Block, style: &ComputedStyle) -> Result<(), TemplateError> {
        match block {
            Block::Paragraph(node) => self.inlines(&node.inlines, style),
            Block::Heading(node) => self.inlines(&node.inlines, style),
            Block::List(node) => {
                for item in &node.items {
                    let meta = &item.meta;
                    let item_style = self.compute(
                        "listItem",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        ComputedStyle::inherited_from(style),
                    )?;
                    let id = self.table.intern(item_style.clone());
                    self.node_style.push((meta.id, id));
                    self.blocks(&item.blocks, &item_style)?;
                }
                Ok(())
            }
            Block::BlockQuote(node) => self.blocks(&node.blocks, style),
            Block::Figure(node) => {
                let meta = &node.image.meta;
                let image_style = self.compute(
                    "image",
                    meta.attrs.role.as_deref(),
                    &class_refs(&meta.attrs.classes),
                    ComputedStyle::inherited_from(style),
                )?;
                let id = self.table.intern(image_style.clone());
                self.node_style.push((meta.id, id));
                self.inlines(&node.caption, style)
            }
            Block::Table(node) => {
                for column in &node.columns {
                    let meta = &column.meta;
                    let column_style = self.compute(
                        "column",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        ComputedStyle::inherited_from(style),
                    )?;
                    let id = self.table.intern(column_style);
                    self.node_style.push((meta.id, id));
                }
                self.inlines(&node.caption, style)?;
                for row in &node.rows {
                    let meta = &row.meta;
                    let row_style = self.compute(
                        "row",
                        meta.attrs.role.as_deref(),
                        &class_refs(&meta.attrs.classes),
                        ComputedStyle::inherited_from(style),
                    )?;
                    let row_id = self.table.intern(row_style.clone());
                    self.node_style.push((meta.id, row_id));
                    for cell in &row.cells {
                        let cell_meta = &cell.meta;
                        let cell_style = self.compute(
                            "cell",
                            cell_meta.attrs.role.as_deref(),
                            &class_refs(&cell_meta.attrs.classes),
                            ComputedStyle::inherited_from(&row_style),
                        )?;
                        let cell_id = self.table.intern(cell_style.clone());
                        self.node_style.push((cell_meta.id, cell_id));
                        self.blocks(&cell.blocks, &cell_style)?;
                    }
                }
                Ok(())
            }
            Block::Section(node) => {
                if let Some(title) = &node.title {
                    self.inlines(title, style)?;
                }
                Ok(())
            }
            Block::Generated(node) => self.blocks(&node.blocks, style),
            Block::CodeBlock(_)
            | Block::MathBlock(_)
            | Block::ThematicBreak(_)
            | Block::PageBreak(_)
            | Block::Raw(_) => Ok(()),
        }
    }

    fn inlines(&mut self, inlines: &[Inline], parent: &ComputedStyle) -> Result<(), TemplateError> {
        for inline in inlines {
            let meta = inline.meta();
            let style = self.compute(
                inline_element(inline),
                meta.attrs.role.as_deref(),
                &class_refs(&meta.attrs.classes),
                ComputedStyle::inherited_from(parent),
            )?;
            let id = self.table.intern(style.clone());
            self.node_style.push((meta.id, id));
            self.inlines(inline.children(), &style)?;
        }
        Ok(())
    }

    fn compute(
        &mut self,
        element: &str,
        role: Option<&str>,
        classes: &[&str],
        mut style: ComputedStyle,
    ) -> Result<ComputedStyle, TemplateError> {
        let mut matched: Vec<(u32, (u32, u32, u32), &StyleRule)> = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.selector.matches(element, role, classes))
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

        if style.align != Align::Justify {
            return Err(TemplateError::new(
                1,
                1,
                "styles",
                "only justify alignment is implemented in model v1",
            ));
        }
        Ok(style)
    }
}

/// Fixed application order: `font-size` first, then `line-height`, then the
/// remaining properties. This makes the result independent of YAML key order.
const PROPERTY_ORDER: [Property; 16] = [
    Property::FontSize,
    Property::LineHeight,
    Property::FirstIndent,
    Property::IndentLeft,
    Property::IndentRight,
    Property::SpaceBefore,
    Property::SpaceAfter,
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
        Inline::LineBreak(_) => "lineBreak",
        Inline::Raw(_) => "rawInline",
    }
}
