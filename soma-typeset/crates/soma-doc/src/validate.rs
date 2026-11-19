//! Structural validation for DocIR.
//!
//! Validation is the only place that knows every invariant. Front ends may rely
//! on it instead of re-deriving rules, and layout may assume a validated
//! document. Paths are stable strings such as `blocks[2].inlines[1]` so
//! diagnostics can point at the offending node.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::attrs::{Attrs, MAX_ATTR_LEN, MAX_CLASSES};
use crate::block::{Block, GeneratedBlock, GeneratedKind, List, Row, Section, Table};
use crate::doc::{Document, FootnoteDef};
use crate::ids::SourceSpan;
use crate::inline::Inline;
use crate::meta::{CoverData, DocumentMeta, ImageRef, MetaField};
use crate::node::NodeMeta;

/// Maximum nesting depth of blocks and inlines. Validation rejects deeper trees
/// before any recursive consumer (canonical writer, text projection, layout)
/// can overflow the stack.
pub const MAX_DEPTH: u32 = 32;

/// One validation failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocError {
    /// Node path, e.g. `blocks[2].inlines[0].href`.
    pub path: String,
    /// Short, stable reason. Not localized.
    pub reason: &'static str,
}

impl DocError {
    pub fn new(path: impl Into<String>, reason: &'static str) -> Self {
        Self {
            path: path.into(),
            reason,
        }
    }
}

impl core::fmt::Display for DocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

/// Validate a document.
pub(crate) fn validate(doc: &Document) -> Result<(), DocError> {
    validate_source_map(doc)?;
    validate_meta(&doc.meta)?;

    let mut context = Context {
        doc,
        ids: Vec::new(),
        anchors: Vec::new(),
        labels: Vec::new(),
        refs: Vec::new(),
    };

    if let Some(title) = &doc.meta.title {
        context.inlines(title, "meta.title", 1)?;
    }
    context.blocks(&doc.blocks, "blocks", 1)?;
    for (index, definition) in doc.footnotes.iter().enumerate() {
        context.footnote(definition, index)?;
    }

    ensure_unique_ids(&mut context.ids)?;
    ensure_unique_strings(&context.anchors, "anchor is duplicated")?;
    ensure_unique_strings(&context.labels, "footnote label is duplicated")?;

    for (target, path) in core::mem::take(&mut context.refs) {
        if doc.find_footnote(&target).is_none() {
            return Err(DocError::new(
                path,
                "footnote reference has no matching definition",
            ));
        }
    }

    Ok(())
}

struct Context<'a> {
    doc: &'a Document,
    ids: Vec<u32>,
    anchors: Vec<String>,
    labels: Vec<String>,
    refs: Vec<(String, String)>,
}

impl Context<'_> {
    fn blocks(&mut self, blocks: &[Block], path: &str, depth: u32) -> Result<(), DocError> {
        check_depth(path, depth)?;
        for (index, block) in blocks.iter().enumerate() {
            self.block(block, &format!("{path}[{index}]"), depth)?;
        }
        Ok(())
    }

    fn block(&mut self, block: &Block, path: &str, depth: u32) -> Result<(), DocError> {
        self.node_meta(block.meta(), path, depth)?;
        let child_depth = depth + 1;

        match block {
            Block::Paragraph(node) => {
                self.inlines(&node.inlines, &format!("{path}.inlines"), child_depth)
            }
            Block::Heading(node) => {
                if !(1..=6).contains(&node.level) {
                    return Err(DocError::new(
                        format!("{path}.level"),
                        "heading level must be in 1..=6",
                    ));
                }
                self.inlines(&node.inlines, &format!("{path}.inlines"), child_depth)
            }
            Block::List(node) => self.list(node, path, child_depth),
            Block::BlockQuote(node) => {
                self.blocks(&node.blocks, &format!("{path}.blocks"), child_depth)
            }
            Block::CodeBlock(node) => {
                if let Some(lang) = &node.lang {
                    check_token(lang, &format!("{path}.lang"), "language tag is invalid")?;
                }
                Ok(())
            }
            Block::MathBlock(node) => require_non_empty_reason(
                &node.source,
                &format!("{path}.source"),
                "math source must not be empty",
            ),
            Block::Figure(node) => {
                validate_image_ref(&node.image.source, &format!("{path}.image"))?;
                self.node_meta(&node.image.meta, &format!("{path}.image"), child_depth)?;
                self.inlines(&node.caption, &format!("{path}.caption"), child_depth)?;
                check_optional_label(&node.label, &format!("{path}.label"))
            }
            Block::Table(node) => self.table(node, path, child_depth),
            Block::ThematicBreak(_) | Block::PageBreak(_) => Ok(()),
            Block::Section(node) => self.section(node, path, child_depth),
            Block::Generated(node) => self.generated(node, path, child_depth),
            Block::Raw(node) => check_token(
                &node.content.format,
                &format!("{path}.format"),
                "raw format must be a non-empty token",
            ),
        }
    }

    fn list(&mut self, list: &List, path: &str, depth: u32) -> Result<(), DocError> {
        if !list.ordered && list.start != 1 {
            return Err(DocError::new(
                format!("{path}.start"),
                "unordered list start must be 1",
            ));
        }
        if list.ordered && list.start == 0 {
            return Err(DocError::new(
                format!("{path}.start"),
                "ordered list start must be >= 1",
            ));
        }
        for (index, item) in list.items.iter().enumerate() {
            let item_path = format!("{path}.items[{index}]");
            self.node_meta(&item.meta, &item_path, depth)?;
            self.blocks(&item.blocks, &format!("{item_path}.blocks"), depth + 1)?;
        }
        Ok(())
    }

    fn table(&mut self, table: &Table, path: &str, depth: u32) -> Result<(), DocError> {
        let expected_columns = table.columns.len();
        if expected_columns == 0 && !table.rows.is_empty() {
            return Err(DocError::new(
                format!("{path}.columns"),
                "table with rows must declare columns",
            ));
        }
        for (index, column) in table.columns.iter().enumerate() {
            self.node_meta(&column.meta, &format!("{path}.columns[{index}]"), depth)?;
        }
        self.inlines(&table.caption, &format!("{path}.caption"), depth)?;
        check_optional_label(&table.label, &format!("{path}.label"))?;

        let rows = table.rows.len();
        let mut occupied = vec![vec![false; expected_columns.max(1)]; rows];
        for (row_index, row) in table.rows.iter().enumerate() {
            let row_path = format!("{path}.rows[{row_index}]");
            self.row_meta(row, &row_path, depth)?;
            if expected_columns == 0 {
                continue;
            }

            let mut column = 0usize;
            for (cell_index, cell) in row.cells.iter().enumerate() {
                let cell_path = format!("{row_path}.cells[{cell_index}]");
                while column < expected_columns && occupied[row_index][column] {
                    column += 1;
                }
                let colspan = cell.colspan as usize;
                let rowspan = cell.rowspan as usize;
                let end_column = column + colspan;
                if end_column > expected_columns {
                    return Err(DocError::new(
                        format!("{cell_path}.colspan"),
                        "cell exceeds the declared columns",
                    ));
                }
                let end_row = row_index + rowspan;
                if end_row > rows {
                    return Err(DocError::new(
                        format!("{cell_path}.rowspan"),
                        "rowspan exceeds the table rows",
                    ));
                }
                for occupied_row in occupied.iter_mut().take(end_row).skip(row_index) {
                    for occupied_cell in occupied_row.iter_mut().take(end_column).skip(column) {
                        if *occupied_cell {
                            return Err(DocError::new(
                                format!("{cell_path}.rowspan"),
                                "cell overlaps another cell",
                            ));
                        }
                        *occupied_cell = true;
                    }
                }
                column = end_column;
            }

            if (0..expected_columns).any(|index| !occupied[row_index][index]) {
                return Err(DocError::new(
                    format!("{row_path}.cells"),
                    "row does not fill the grid; colspan sum must equal the column count",
                ));
            }
        }
        Ok(())
    }

    fn row_meta(&mut self, row: &Row, path: &str, depth: u32) -> Result<(), DocError> {
        self.node_meta(&row.meta, path, depth)?;
        for (index, cell) in row.cells.iter().enumerate() {
            let cell_path = format!("{path}.cells[{index}]");
            self.node_meta(&cell.meta, &cell_path, depth)?;
            if cell.colspan == 0 {
                return Err(DocError::new(
                    format!("{cell_path}.colspan"),
                    "colspan must be >= 1",
                ));
            }
            if cell.rowspan == 0 {
                return Err(DocError::new(
                    format!("{cell_path}.rowspan"),
                    "rowspan must be >= 1",
                ));
            }
            self.blocks(&cell.blocks, &format!("{cell_path}.blocks"), depth + 1)?;
        }
        Ok(())
    }

    fn section(&mut self, section: &Section, path: &str, depth: u32) -> Result<(), DocError> {
        if !(1..=6).contains(&section.level) {
            return Err(DocError::new(
                format!("{path}.level"),
                "section level must be in 1..=6",
            ));
        }
        if let Some(template) = &section.page_template {
            check_token(
                template,
                &format!("{path}.pageTemplate"),
                "page template name must be a non-empty token",
            )?;
        }
        if let Some(title) = &section.title {
            self.inlines(title, &format!("{path}.title"), depth)?;
        }
        Ok(())
    }

    fn generated(
        &mut self,
        generated: &GeneratedBlock,
        path: &str,
        depth: u32,
    ) -> Result<(), DocError> {
        if let Some(slot) = &generated.slot {
            check_token(
                slot,
                &format!("{path}.slot"),
                "slot must be a non-empty token",
            )?;
        } else if generated.kind == GeneratedKind::Custom {
            return Err(DocError::new(
                format!("{path}.slot"),
                "custom generated block requires a slot",
            ));
        }
        self.blocks(&generated.blocks, &format!("{path}.blocks"), depth)
    }

    fn inlines(&mut self, inlines: &[Inline], path: &str, depth: u32) -> Result<(), DocError> {
        check_depth(path, depth)?;
        for (index, inline) in inlines.iter().enumerate() {
            self.inline(inline, &format!("{path}[{index}]"), depth)?;
        }
        Ok(())
    }

    fn inline(&mut self, inline: &Inline, path: &str, depth: u32) -> Result<(), DocError> {
        self.node_meta(inline.meta(), path, depth)?;
        let child_depth = depth + 1;

        match inline {
            Inline::Text(node) => require_non_empty(&node.value, &format!("{path}.text")),
            Inline::Code(node) => require_non_empty(&node.value, &format!("{path}.text")),
            Inline::Strong(_)
            | Inline::Emphasis(_)
            | Inline::Underline(_)
            | Inline::Strikethrough(_)
            | Inline::SmallCaps(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_) => {
                self.inlines(inline.children(), &format!("{path}.inlines"), child_depth)
            }
            Inline::Link(node) => {
                require_non_empty(&node.href, &format!("{path}.href"))?;
                self.inlines(&node.children, &format!("{path}.inlines"), child_depth)
            }
            Inline::Image(node) => validate_image_ref(&node.source, path),
            Inline::Math(node) => require_non_empty_reason(
                &node.source,
                &format!("{path}.source"),
                "math source must not be empty",
            ),
            Inline::FootnoteRef(node) => {
                require_non_empty(&node.target, &format!("{path}.target"))?;
                self.refs
                    .push((node.target.clone(), format!("{path}.target")));
                Ok(())
            }
            Inline::Ruby(node) => {
                require_non_empty(&node.base, &format!("{path}.base"))?;
                require_non_empty(&node.annotation, &format!("{path}.annotation"))
            }
            Inline::Index(node) => require_non_empty_reason(
                &node.term,
                &format!("{path}.term"),
                "index term must not be empty",
            ),
            Inline::LineBreak(_) => Ok(()),
            Inline::Raw(node) => check_token(
                &node.content.format,
                &format!("{path}.format"),
                "raw format must be a non-empty token",
            ),
        }
    }

    fn footnote(&mut self, definition: &FootnoteDef, index: usize) -> Result<(), DocError> {
        let path = format!("footnotes[{index}]");
        self.node_meta(&definition.meta, &path, 1)?;
        check_token(
            &definition.label,
            &format!("{path}.label"),
            "footnote label must be a non-empty token",
        )?;
        self.labels.push(definition.label.clone());
        self.blocks(&definition.blocks, &format!("{path}.blocks"), 2)
    }

    fn node_meta(&mut self, meta: &NodeMeta, path: &str, depth: u32) -> Result<(), DocError> {
        check_depth(path, depth)?;
        if !meta.id.is_assigned() {
            return Err(DocError::new(path, "node id must be assigned"));
        }
        self.ids.push(meta.id.0);
        self.span(meta.span, path)?;
        self.attrs(&meta.attrs, path)
    }

    fn span(&self, span: SourceSpan, path: &str) -> Result<(), DocError> {
        if span.start > span.end {
            return Err(DocError::new(
                format!("{path}.span"),
                "span start must not exceed span end",
            ));
        }
        if span.source.is_known() && !self.doc.source_map.contains(span.source) {
            return Err(DocError::new(
                format!("{path}.span.source"),
                "span references an unknown source",
            ));
        }
        Ok(())
    }

    fn attrs(&mut self, attrs: &Attrs, path: &str) -> Result<(), DocError> {
        if let Some(role) = &attrs.role {
            check_attr(role, &format!("{path}.attrs.role"), "role is invalid")?;
        }
        if attrs.classes.len() > MAX_CLASSES {
            return Err(DocError::new(
                format!("{path}.attrs.classes"),
                "too many classes",
            ));
        }
        for (index, class) in attrs.classes.iter().enumerate() {
            check_attr(
                class,
                &format!("{path}.attrs.classes[{index}]"),
                "class is invalid",
            )?;
            if attrs.classes[..index].iter().any(|seen| seen == class) {
                return Err(DocError::new(
                    format!("{path}.attrs.classes[{index}]"),
                    "class is duplicated",
                ));
            }
        }
        if let Some(anchor) = &attrs.anchor {
            check_attr(anchor, &format!("{path}.attrs.anchor"), "anchor is invalid")?;
            self.anchors.push(anchor.clone());
        }
        if let Some(lang) = &attrs.lang {
            check_attr(
                lang,
                &format!("{path}.attrs.lang"),
                "language tag is invalid",
            )?;
        }
        Ok(())
    }
}

fn check_depth(path: &str, depth: u32) -> Result<(), DocError> {
    if depth > MAX_DEPTH {
        return Err(DocError::new(path, "nesting depth exceeds MAX_DEPTH"));
    }
    Ok(())
}

fn validate_source_map(doc: &Document) -> Result<(), DocError> {
    let mut ids: Vec<u16> = Vec::with_capacity(doc.source_map.len());
    for (index, entry) in doc.source_map.entries.iter().enumerate() {
        let path = format!("sourceMap[{index}]");
        if !entry.id.is_known() {
            return Err(DocError::new(
                format!("{path}.id"),
                "source id 0 is reserved for unknown",
            ));
        }
        require_non_empty(&entry.name, &format!("{path}.name"))?;
        ids.push(entry.id.0);
    }
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(DocError::new("sourceMap", "source id is duplicated"));
    }
    Ok(())
}

fn validate_meta(meta: &DocumentMeta) -> Result<(), DocError> {
    if let Some(language) = &meta.language {
        check_token(language, "meta.language", "language tag is invalid")?;
    }
    if let Some(date) = meta.date {
        if !date.is_in_range() {
            return Err(DocError::new("meta.date", "date is out of range"));
        }
    }
    for (index, keyword) in meta.keywords.iter().enumerate() {
        require_non_empty(keyword, &format!("meta.keywords[{index}]"))?;
    }
    ensure_unique_strings(&meta.keywords, "keyword is duplicated")?;
    validate_fields(&meta.fields, "meta.fields")?;
    for (index, author) in meta.authors.iter().enumerate() {
        require_non_empty(&author.name, &format!("meta.authors[{index}].name"))?;
        if let Some(affiliation) = &author.affiliation {
            require_non_empty(affiliation, &format!("meta.authors[{index}].affiliation"))?;
        }
    }
    if let Some(template) = &meta.template {
        check_token(
            &template.name,
            "meta.template.name",
            "template name is invalid",
        )?;
        if let Some(version) = &template.version {
            check_token(
                version,
                "meta.template.version",
                "template version is invalid",
            )?;
        }
    }
    if let Some(cover) = &meta.cover {
        validate_cover(cover)?;
    }
    Ok(())
}

fn validate_cover(cover: &CoverData) -> Result<(), DocError> {
    if let Some(title) = &cover.title {
        require_non_empty(title, "meta.cover.title")?;
    }
    if let Some(subtitle) = &cover.subtitle {
        require_non_empty(subtitle, "meta.cover.subtitle")?;
    }
    if let Some(date) = &cover.date {
        require_non_empty(date, "meta.cover.date")?;
    }
    for (index, author) in cover.authors.iter().enumerate() {
        require_non_empty(author, &format!("meta.cover.authors[{index}]"))?;
    }
    if let Some(logo) = &cover.logo {
        validate_image_ref(logo, "meta.cover.logo")?;
    }
    validate_fields(&cover.fields, "meta.cover.fields")
}

fn validate_fields(fields: &[MetaField], path: &str) -> Result<(), DocError> {
    for (index, field) in fields.iter().enumerate() {
        check_token(
            &field.key,
            &format!("{path}[{index}].key"),
            "field key is invalid",
        )?;
        if fields[..index].iter().any(|seen| seen.key == field.key) {
            return Err(DocError::new(
                format!("{path}[{index}].key"),
                "field key is duplicated",
            ));
        }
    }
    Ok(())
}

fn validate_image_ref(image: &ImageRef, path: &str) -> Result<(), DocError> {
    require_non_empty(&image.src, &format!("{path}.src"))?;
    if let Some(intrinsic) = image.intrinsic {
        if intrinsic.width_px == 0 || intrinsic.height_px == 0 {
            return Err(DocError::new(
                format!("{path}.intrinsic"),
                "intrinsic size must be positive",
            ));
        }
    }
    Ok(())
}

fn check_optional_label(label: &Option<String>, path: &str) -> Result<(), DocError> {
    if let Some(label) = label {
        check_token(label, path, "label must be a non-empty token")?;
    }
    Ok(())
}

fn require_non_empty(value: &str, path: &str) -> Result<(), DocError> {
    require_non_empty_reason(value, path, "value must not be empty")
}

fn require_non_empty_reason(value: &str, path: &str, reason: &'static str) -> Result<(), DocError> {
    if value.is_empty() {
        return Err(DocError::new(path, reason));
    }
    Ok(())
}

fn check_attr(value: &str, path: &str, reason: &'static str) -> Result<(), DocError> {
    check_token(value, path, reason)?;
    if value.len() > MAX_ATTR_LEN {
        return Err(DocError::new(path, "attribute value is too long"));
    }
    Ok(())
}

/// Tokens have no whitespace and no control characters.
fn check_token(value: &str, path: &str, reason: &'static str) -> Result<(), DocError> {
    if value.is_empty() {
        return Err(DocError::new(path, reason));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(DocError::new(path, reason));
    }
    Ok(())
}

fn ensure_unique_ids(ids: &mut [u32]) -> Result<(), DocError> {
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(DocError::new("document", "node id is duplicated"));
    }
    Ok(())
}

fn ensure_unique_strings(values: &[String], reason: &'static str) -> Result<(), DocError> {
    let duplicated = values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].iter().any(|seen| seen == value));
    if duplicated {
        return Err(DocError::new("document", reason));
    }
    Ok(())
}
