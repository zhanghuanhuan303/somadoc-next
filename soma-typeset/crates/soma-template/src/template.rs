//! Template model and its YAML parser.
//!
//! A template is our DSL expressed in a YAML subset (`crate::yaml`). It owns
//! presentation only: page geometry, style rules, footnote numbering. It cannot
//! write engine knobs (tolerance, weights, pagination) — those keys are unknown
//! here and therefore errors.

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::Align;

use crate::units::{LengthSpec, LineHeightSpec, Rational, Unit};
use crate::yaml::{Key, Value, ValueKind};
use crate::TemplateError;

pub const SCHEMA: &str = "soma.template/4";

/// Element names a selector may target. Derived from DocIR node kinds; a typo is
/// an error instead of silently matching nothing.
pub const ELEMENTS: &[&str] = &[
    "document",
    "title",
    "paragraph",
    "heading",
    "list",
    "listItem",
    "blockQuote",
    "codeBlock",
    "mathBlock",
    "figure",
    "table",
    "column",
    "row",
    "cell",
    "thematicBreak",
    "pageBreak",
    "section",
    "generated",
    "rawBlock",
    "footnote",
    "text",
    "code",
    "strong",
    "emphasis",
    "underline",
    "strikethrough",
    "smallCaps",
    "superscript",
    "subscript",
    "link",
    "image",
    "math",
    "footnoteRef",
    "ruby",
    "lineBreak",
    "rawInline",
];

/// A parsed, validated template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub version: Option<String>,
    pub page: PageSpec,
    pub footnote: FootnoteSpec,
    pub table: TableSpec,
    pub styles: Vec<StyleRule>,
}

impl Template {
    /// Root font size in points, taken from the `document` rule. Required by the
    /// unit system: `1em` is defined by the body text.
    pub fn body_font_pt(&self) -> Rational {
        let mut found = None;
        for rule in &self.styles {
            if rule.selector.element.as_deref() != Some("document") {
                continue;
            }
            for declaration in &rule.declarations {
                if declaration.property != Property::FontSize {
                    continue;
                }
                if let PropertyValue::Length(length) = &declaration.value {
                    if let Some(points) = length.absolute_points() {
                        found = Some(points);
                    }
                }
            }
        }
        found.unwrap_or(Rational::ONE)
    }

    /// Canonical JSON of the template.
    pub fn canonical_json(&self) -> String {
        crate::canonical::template_json(self)
    }

    /// Hash of the canonical JSON; the style layer records it.
    pub fn template_hash(&self) -> crate::TemplateHash {
        crate::canonical::template_hash(self)
    }
}

/// Page geometry. Sizes and margins are lengths; they resolve against the root
/// font size like every other physical length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSpec {
    pub size: PageSize,
    pub margin_top: LengthSpec,
    pub margin_bottom: LengthSpec,
    pub margin_left: LengthSpec,
    pub margin_right: LengthSpec,
    /// Height of the running header band, drawn inside the top margin.
    pub header: LengthSpec,
    /// Height of the footer band, drawn below the content box.
    pub footer: LengthSpec,
    /// `running-title` (default) or a literal string.
    pub header_text: Option<String>,
    /// Folio template: `{page}` and `{pages}` are substituted.
    pub folio: Option<String>,
}

/// Named page size or an explicit pair of lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    A4,
    A5,
    Letter,
    Custom {
        width: LengthSpec,
        height: LengthSpec,
    },
}

impl PageSize {
    /// Width and height in that order.
    pub fn dimensions(self) -> (LengthSpec, LengthSpec) {
        let mm = |value: i64| {
            LengthSpec::new(
                Rational::new(value, 1).expect("non-zero denominator"),
                Unit::Mm,
            )
        };
        let inches = |tenths: i64| {
            LengthSpec::new(
                Rational::new(tenths, 10).expect("non-zero denominator"),
                Unit::In,
            )
        };
        match self {
            Self::A4 => (mm(210), mm(297)),
            Self::A5 => (mm(148), mm(210)),
            Self::Letter => (inches(85), inches(110)),
            Self::Custom { width, height } => (width, height),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::Letter => "Letter",
            Self::Custom { .. } => "custom",
        }
    }
}

/// Footnote numbering policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FootnoteNumbering {
    /// Continuous through the document.
    #[default]
    Document,
    /// Restart on every page.
    Page,
    /// Restart at every section.
    Section,
}

impl FootnoteNumbering {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Page => "page",
            Self::Section => "section",
        }
    }
}

/// Footnote settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FootnoteSpec {
    pub numbering: FootnoteNumbering,
}

/// Table rule thicknesses. Zero draws no rule at that boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSpec {
    /// Rule between two body rows.
    pub rule: LengthSpec,
    /// Rule below the last header row.
    pub header_rule: LengthSpec,
    /// Rule above row 0 and below the last row.
    pub outer_rule: LengthSpec,
    /// Vertical rule between two body columns.
    pub column_rule: LengthSpec,
    /// Vertical rule left of column 0 and right of the last column.
    pub outer_column_rule: LengthSpec,
    /// Solid or dashed.
    pub style: RuleStyle,
    /// Packed `0xRRGGBB`; `None` uses the table style's color.
    pub color: Option<u32>,
}

/// How a table rule is painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuleStyle {
    #[default]
    Solid,
    Dashed,
}

impl RuleStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "solid" => Some(Self::Solid),
            "dashed" => Some(Self::Dashed),
            _ => None,
        }
    }
}

impl Default for TableSpec {
    fn default() -> Self {
        Self {
            rule: LengthSpec::zero(),
            header_rule: LengthSpec::zero(),
            outer_rule: LengthSpec::zero(),
            column_rule: LengthSpec::zero(),
            outer_column_rule: LengthSpec::zero(),
            style: RuleStyle::Solid,
            color: None,
        }
    }
}

impl TableSpec {
    /// No rule is drawn anywhere; the table keeps its rule-free geometry.
    pub fn is_zero(self) -> bool {
        self.rule.value == Rational::ZERO
            && self.header_rule.value == Rational::ZERO
            && self.outer_rule.value == Rational::ZERO
            && self.column_rule.value == Rational::ZERO
            && self.outer_column_rule.value == Rational::ZERO
    }
}

/// One style rule: a selector plus declarations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleRule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

/// Selector. Every present part must match (`element` + `role` + all `classes`).
/// `within` additionally requires some block ancestor to be that element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selector {
    pub element: Option<String>,
    pub role: Option<String>,
    pub classes: Vec<String>,
    pub within: Option<String>,
}

impl Selector {
    pub fn matches(
        &self,
        element: &str,
        role: Option<&str>,
        classes: &[&str],
        ancestors: &[&str],
    ) -> bool {
        if let Some(expected) = &self.element {
            if expected != element {
                return false;
            }
        }
        if let Some(expected) = &self.role {
            if role != Some(expected.as_str()) {
                return false;
            }
        }
        if let Some(expected) = &self.within {
            if !ancestors.iter().any(|ancestor| ancestor == expected) {
                return false;
            }
        }
        self.classes
            .iter()
            .all(|expected| classes.iter().any(|actual| actual == expected))
    }

    /// Higher is more specific. Ties are resolved by declaration order.
    pub(crate) fn specificity(&self) -> (u32, u32, u32) {
        (
            self.classes.len() as u32,
            if self.role.is_some() { 1 } else { 0 } + if self.within.is_some() { 1 } else { 0 },
            if self.element.is_some() { 1 } else { 0 },
        )
    }
}

/// A property name. The whitelist is the contract: unknown keys are errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    FontSize,
    LineHeight,
    FirstIndent,
    IndentLeft,
    IndentRight,
    SpaceBefore,
    SpaceAfter,
    SpaceBeforePlus,
    SpaceBeforeMinus,
    SpaceAfterPlus,
    SpaceAfterMinus,
    WidowLines,
    OrphanLines,
    HangingPunctuation,
    HangingCap,
    BaselineGrid,
    Align,
    Family,
    Color,
    Weight,
    Slant,
    Underline,
    SmallCaps,
    KeepWithNext,
    PageBreakBefore,
}

impl Property {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FontSize => "font-size",
            Self::LineHeight => "line-height",
            Self::FirstIndent => "first-indent",
            Self::IndentLeft => "indent-left",
            Self::IndentRight => "indent-right",
            Self::SpaceBefore => "space-before",
            Self::SpaceAfter => "space-after",
            Self::SpaceBeforePlus => "space-before-plus",
            Self::SpaceBeforeMinus => "space-before-minus",
            Self::SpaceAfterPlus => "space-after-plus",
            Self::SpaceAfterMinus => "space-after-minus",
            Self::WidowLines => "widow-lines",
            Self::OrphanLines => "orphan-lines",
            Self::HangingPunctuation => "hanging-punctuation",
            Self::HangingCap => "hanging-cap",
            Self::BaselineGrid => "baseline-grid",
            Self::Align => "align",
            Self::Family => "family",
            Self::Color => "color",
            Self::Weight => "weight",
            Self::Slant => "slant",
            Self::Underline => "underline",
            Self::SmallCaps => "small-caps",
            Self::KeepWithNext => "keep-with-next",
            Self::PageBreakBefore => "page-break-before",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "font-size" => Self::FontSize,
            "line-height" => Self::LineHeight,
            "first-indent" => Self::FirstIndent,
            "indent-left" => Self::IndentLeft,
            "indent-right" => Self::IndentRight,
            "space-before" => Self::SpaceBefore,
            "space-after" => Self::SpaceAfter,
            "space-before-plus" => Self::SpaceBeforePlus,
            "space-before-minus" => Self::SpaceBeforeMinus,
            "space-after-plus" => Self::SpaceAfterPlus,
            "space-after-minus" => Self::SpaceAfterMinus,
            "widow-lines" => Self::WidowLines,
            "orphan-lines" => Self::OrphanLines,
            "hanging-punctuation" => Self::HangingPunctuation,
            "hanging-cap" => Self::HangingCap,
            "baseline-grid" => Self::BaselineGrid,
            "align" => Self::Align,
            "family" => Self::Family,
            "color" => Self::Color,
            "weight" => Self::Weight,
            "slant" => Self::Slant,
            "underline" => Self::Underline,
            "small-caps" => Self::SmallCaps,
            "keep-with-next" => Self::KeepWithNext,
            "page-break-before" => Self::PageBreakBefore,
            _ => return None,
        })
    }
}

/// Typed declaration value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyValue {
    Length(LengthSpec),
    LineHeight(LineHeightSpec),
    Align(Align),
    Bool(bool),
    /// Small non-negative integer (`widow-lines`, `orphan-lines`).
    Count(u8),
    Weight(FontWeight),
    Slant(FontSlant),
    Color(u32),
    /// Font family token list, resolved by the renderer.
    Text(String),
}

/// One declaration with its source position, so semantic errors point at the
/// line the author wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub property: Property,
    pub value: PropertyValue,
    pub line: u32,
    pub column: u32,
}

/// Font weight from the template.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
}

/// Font slant from the template.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontSlant {
    #[default]
    Normal,
    Italic,
}

/// Parse and validate a template.
pub fn parse_template(source: &str) -> Result<Template, TemplateError> {
    let root = crate::yaml::parse(source)?;
    let fields = Fields::new(&root, "")?;
    fields.reject_unknown(&["name", "version", "page", "footnote", "table", "styles"])?;

    let name = fields.require_str("name")?;
    check_token(&name, fields.error("name must be a non-empty token"))?;
    let version = fields.optional_str("version")?;

    let page = parse_page(fields.require("page")?, "page")?;
    let footnote = match fields.optional("footnote") {
        Some(value) if !value.is_null() => parse_footnote(value, "footnote")?,
        _ => FootnoteSpec::default(),
    };
    let table = match fields.optional("table") {
        Some(value) if !value.is_null() => parse_table(value, "table")?,
        _ => TableSpec::default(),
    };

    let style_nodes = fields.require("styles")?.as_seq("styles")?;
    let mut styles = Vec::with_capacity(style_nodes.len());
    for (index, node) in style_nodes.iter().enumerate() {
        styles.push(parse_rule(node, index)?);
    }

    let template = Template {
        name,
        version,
        page,
        footnote,
        table,
        styles,
    };
    validate_template(&template)?;
    Ok(template)
}

fn validate_template(template: &Template) -> Result<(), TemplateError> {
    if template.styles.is_empty() {
        return Err(TemplateError::new(
            1,
            1,
            "styles",
            "template needs at least one rule",
        ));
    }

    let mut has_document_font = false;
    for (index, rule) in template.styles.iter().enumerate() {
        let path = alloc::format!("styles[{index}]");
        if rule.declarations.is_empty() {
            return Err(TemplateError::new(1, 1, &path, "rule has no declarations"));
        }
        if rule.selector.element.as_deref() != Some("document") {
            continue;
        }
        for declaration in &rule.declarations {
            if declaration.property != Property::FontSize {
                continue;
            }
            match &declaration.value {
                PropertyValue::Length(length) => match length.absolute_points() {
                    Some(_) => has_document_font = true,
                    None => {
                        return Err(TemplateError::new(
                            declaration.line,
                            declaration.column,
                            &path,
                            "document font-size must use an absolute unit",
                        ))
                    }
                },
                _ => {
                    return Err(TemplateError::new(
                        declaration.line,
                        declaration.column,
                        &path,
                        "document font-size must use an absolute unit",
                    ))
                }
            }
        }
    }

    if !has_document_font {
        return Err(TemplateError::new(
            1,
            1,
            "styles",
            "template must set font-size on the document element",
        ));
    }
    Ok(())
}

fn parse_page(value: &Value, path: &str) -> Result<PageSpec, TemplateError> {
    let fields = Fields::new(value, path)?;
    fields.reject_unknown(&[
        "size",
        "width",
        "height",
        "margin",
        "margin-top",
        "margin-bottom",
        "margin-left",
        "margin-right",
        "header",
        "footer",
        "header-text",
        "folio",
    ])?;

    let size_text = fields.require_str("size")?;
    let size = match size_text.as_str() {
        "A4" => PageSize::A4,
        "A5" => PageSize::A5,
        "letter" | "Letter" => PageSize::Letter,
        "custom" => {
            let width = parse_length(fields.require("width")?, "page")?;
            let height = parse_length(fields.require("height")?, "page")?;
            if width.is_em() || height.is_em() {
                return Err(fields.error("custom page size must use absolute units"));
            }
            PageSize::Custom { width, height }
        }
        _ => return Err(fields.error("page size must be A4, A5, Letter, or custom")),
    };

    let default_margin = LengthSpec::new(
        Rational::new(20, 1).expect("non-zero denominator"),
        Unit::Mm,
    );
    let shared = fields
        .optional("margin")
        .map(|value| parse_length(value, "page"))
        .transpose()?;

    let margin_top = fields
        .optional("margin-top")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .or(shared)
        .unwrap_or(default_margin);
    let margin_bottom = fields
        .optional("margin-bottom")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .or(shared)
        .unwrap_or(default_margin);
    let margin_left = fields
        .optional("margin-left")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .or(shared)
        .unwrap_or(default_margin);
    let margin_right = fields
        .optional("margin-right")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .or(shared)
        .unwrap_or(default_margin);

    let zero = LengthSpec::new(Rational::new(0, 1).expect("non-zero denominator"), Unit::Mm);
    let header = fields
        .optional("header")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .unwrap_or(zero);
    let footer = fields
        .optional("footer")
        .map(|value| parse_length(value, "page"))
        .transpose()?
        .unwrap_or(zero);
    let header_text = fields.optional_str("header-text")?;
    let folio = fields.optional_str("folio")?;

    Ok(PageSpec {
        size,
        margin_top,
        margin_bottom,
        margin_left,
        margin_right,
        header,
        footer,
        header_text,
        folio,
    })
}

fn parse_footnote(value: &Value, path: &str) -> Result<FootnoteSpec, TemplateError> {
    let fields = Fields::new(value, path)?;
    fields.reject_unknown(&["numbering"])?;
    let numbering = match fields.optional_str("numbering")?.as_deref() {
        None | Some("document") => FootnoteNumbering::Document,
        Some("page") => FootnoteNumbering::Page,
        Some("section") => FootnoteNumbering::Section,
        Some(_) => {
            return Err(fields.error("footnote numbering must be document, page, or section"))
        }
    };
    Ok(FootnoteSpec { numbering })
}

fn parse_table(value: &Value, path: &str) -> Result<TableSpec, TemplateError> {
    let fields = Fields::new(value, path)?;
    fields.reject_unknown(&[
        "rule-width",
        "header-rule-width",
        "outer-rule-width",
        "column-rule-width",
        "outer-column-rule-width",
        "rule-style",
        "rule-color",
    ])?;

    let mut spec = TableSpec::default();
    let read = |name: &str| -> Result<Option<LengthSpec>, TemplateError> {
        fields
            .optional(name)
            .map(|value| parse_length(value, path))
            .transpose()
    };
    if let Some(length) = read("rule-width")? {
        spec.rule = length;
    }
    if let Some(length) = read("header-rule-width")? {
        spec.header_rule = length;
    }
    if let Some(length) = read("outer-rule-width")? {
        spec.outer_rule = length;
    }
    if let Some(length) = read("column-rule-width")? {
        spec.column_rule = length;
    }
    if let Some(length) = read("outer-column-rule-width")? {
        spec.outer_column_rule = length;
    }
    if let Some(text) = fields.optional_str("rule-style")? {
        spec.style = RuleStyle::parse(&text)
            .ok_or_else(|| fields.error("rule style must be solid or dashed"))?;
    }
    if let Some(text) = fields.optional_str("rule-color")? {
        spec.color =
            Some(parse_color(&text).ok_or_else(|| fields.error("rule color must be #rrggbb"))?);
    }

    for length in [
        spec.rule,
        spec.header_rule,
        spec.outer_rule,
        spec.column_rule,
        spec.outer_column_rule,
    ] {
        if length.value.num() < 0 {
            return Err(fields.error("table rule width must not be negative"));
        }
    }

    Ok(spec)
}

fn parse_rule(value: &Value, index: usize) -> Result<StyleRule, TemplateError> {
    let path = alloc::format!("styles[{index}]");
    let fields = Fields::new(value, &path)?;
    fields.reject_unknown(&["selector", "declarations"])?;

    let selector = parse_selector(fields.require("selector")?, &path)?;
    let declaration_fields = Fields::new(fields.require("declarations")?, &path)?;
    if declaration_fields.entries.is_empty() {
        return Err(fields.error("rule has no declarations"));
    }

    let mut declarations = Vec::with_capacity(declaration_fields.entries.len());
    for (key, raw) in declaration_fields.entries {
        let property_enum = Property::parse(&key.name).ok_or_else(|| {
            TemplateError::new(key.line, key.column, &path, "unknown style property")
        })?;
        declarations.push(parse_declaration(property_enum, raw, key, &path)?);
    }

    Ok(StyleRule {
        selector,
        declarations,
    })
}

fn parse_selector(value: &Value, path: &str) -> Result<Selector, TemplateError> {
    let fields = Fields::new(value, path)?;
    fields.reject_unknown(&["element", "role", "class", "classes", "within"])?;

    let element = fields.optional_str("element")?;
    if let Some(element) = &element {
        if !ELEMENTS.contains(&element.as_str()) {
            return Err(fields.error("unknown element selector"));
        }
    }

    let within = fields.optional_str("within")?;
    if let Some(within) = &within {
        if !ELEMENTS.contains(&within.as_str()) {
            return Err(fields.error("unknown within selector"));
        }
    }

    let role = fields.optional_str("role")?;
    if let Some(role) = &role {
        check_token(role, fields.error("role must be a non-empty token"))?;
    }

    let mut classes = Vec::new();
    if let Some(class) = fields.optional_str("class")? {
        check_token(&class, fields.error("class must be a non-empty token"))?;
        classes.push(class);
    }
    if let Some(classes_value) = fields.optional("classes") {
        for item in classes_value.as_seq(path)? {
            let class = item.as_str(path)?;
            check_token(&class, item.error(path, "class must be a non-empty token"))?;
            classes.push(class);
        }
    }
    for (index, class) in classes.iter().enumerate() {
        if classes[..index].iter().any(|seen| seen == class) {
            return Err(fields.error("duplicate class in selector"));
        }
    }

    if element.is_none() && role.is_none() && classes.is_empty() {
        return Err(fields.error("selector must name an element, role, or class"));
    }

    Ok(Selector {
        element,
        role,
        classes,
        within,
    })
}

fn parse_declaration(
    property: Property,
    value: &Value,
    key: &Key,
    path: &str,
) -> Result<Declaration, TemplateError> {
    let text = value.as_str(path)?;
    let error = |reason: &'static str| TemplateError::new(key.line, key.column, path, reason);

    let parsed = match property {
        Property::FontSize
        | Property::FirstIndent
        | Property::IndentLeft
        | Property::IndentRight
        | Property::SpaceBefore
        | Property::SpaceAfter
        | Property::SpaceBeforePlus
        | Property::SpaceBeforeMinus
        | Property::SpaceAfterPlus
        | Property::SpaceAfterMinus
        | Property::HangingCap
        | Property::BaselineGrid => {
            let length = LengthSpec::parse(&text).ok_or_else(|| error("invalid length value"))?;
            PropertyValue::Length(length)
        }
        Property::LineHeight => {
            if let Some(multiple) = Rational::parse(&text) {
                PropertyValue::LineHeight(LineHeightSpec::Multiple(multiple))
            } else {
                let length =
                    LengthSpec::parse(&text).ok_or_else(|| error("invalid line-height value"))?;
                PropertyValue::LineHeight(LineHeightSpec::Length(length))
            }
        }
        Property::Align => PropertyValue::Align(match text.as_str() {
            "justify" => Align::Justify,
            "left" => Align::Left,
            "center" => Align::Center,
            "right" => Align::Right,
            _ => return Err(error("align must be justify, left, center, or right")),
        }),
        Property::Family => {
            if text.is_empty() {
                return Err(error("family must not be empty"));
            }
            PropertyValue::Text(text)
        }
        Property::Color => {
            let color = parse_color(&text).ok_or_else(|| error("color must be #rrggbb"))?;
            PropertyValue::Color(color)
        }
        Property::Weight => PropertyValue::Weight(match text.as_str() {
            "normal" => FontWeight::Normal,
            "bold" => FontWeight::Bold,
            _ => return Err(error("weight must be normal or bold")),
        }),
        Property::Slant => PropertyValue::Slant(match text.as_str() {
            "normal" => FontSlant::Normal,
            "italic" => FontSlant::Italic,
            _ => return Err(error("slant must be normal or italic")),
        }),
        Property::Underline
        | Property::SmallCaps
        | Property::KeepWithNext
        | Property::PageBreakBefore
        | Property::HangingPunctuation => {
            let flag = match text.as_str() {
                "true" | "yes" => true,
                "false" | "no" => false,
                _ => return Err(error("expected true or false")),
            };
            PropertyValue::Bool(flag)
        }
        Property::WidowLines | Property::OrphanLines => {
            let count = text
                .parse::<u8>()
                .map_err(|_| error("expected a non-negative integer"))?;
            if count > 9 {
                return Err(error("expected a count between 0 and 9"));
            }
            PropertyValue::Count(count)
        }
    };

    Ok(Declaration {
        property,
        value: parsed,
        line: key.line,
        column: key.column,
    })
}

fn parse_length(value: &Value, path: &str) -> Result<LengthSpec, TemplateError> {
    let text = value.as_str(path)?;
    LengthSpec::parse(&text).ok_or_else(|| value.error(path, "invalid length value"))
}

fn parse_color(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let bytes = digits.as_bytes();
    if bytes.len() != 6 {
        return None;
    }
    let mut value = 0u32;
    for byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        value = value * 16 + digit as u32;
    }
    Some(value)
}

fn check_token(value: &str, error: TemplateError) -> Result<(), TemplateError> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(error);
    }
    Ok(())
}

/// Typed field access with path and position for diagnostics.
struct Fields<'a> {
    value: &'a Value,
    entries: &'a [(Key, Value)],
    path: &'a str,
}

impl<'a> Fields<'a> {
    fn new(value: &'a Value, path: &'a str) -> Result<Self, TemplateError> {
        let entries = match &value.kind {
            ValueKind::Map(entries) => entries.as_slice(),
            _ => return Err(value.error(path, "expected a mapping")),
        };
        Ok(Self {
            value,
            entries,
            path,
        })
    }

    fn error(&self, reason: &'static str) -> TemplateError {
        self.value.error(self.path, reason)
    }

    fn reject_unknown(&self, allowed: &[&str]) -> Result<(), TemplateError> {
        for (key, _) in self.entries {
            if !allowed.contains(&key.name.as_str()) {
                return Err(TemplateError::new(
                    key.line,
                    key.column,
                    self.path,
                    "unknown key",
                ));
            }
        }
        Ok(())
    }

    fn optional(&self, name: &str) -> Option<&'a Value> {
        self.entries
            .iter()
            .find(|(key, _)| key.name == name)
            .map(|(_, value)| value)
    }

    fn require(&self, name: &str) -> Result<&'a Value, TemplateError> {
        self.optional(name)
            .ok_or_else(|| self.error("missing required key"))
    }

    fn require_str(&self, name: &str) -> Result<String, TemplateError> {
        let value = self.require(name)?;
        let text = value.as_str(self.path)?;
        if text.is_empty() {
            return Err(value.error(self.path, "value must not be empty"));
        }
        Ok(text)
    }

    fn optional_str(&self, name: &str) -> Result<Option<String>, TemplateError> {
        match self.optional(name) {
            None => Ok(None),
            Some(value) if value.is_null() => Ok(None),
            Some(value) => {
                let text = value.as_str(self.path)?;
                if text.is_empty() {
                    return Err(value.error(self.path, "value must not be empty"));
                }
                Ok(Some(text))
            }
        }
    }
}
