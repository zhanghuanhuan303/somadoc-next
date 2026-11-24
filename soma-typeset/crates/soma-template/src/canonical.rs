//! Canonical JSON and hashes for templates and resolved styles.
//!
//! Same rules as every other layer: one hand-written writer, fixed field order,
//! integers only, strings escaped by the JSON rules. `soma.template/1` for the
//! template, `soma.styles/1` for the resolved style layer.

use alloc::string::String;
use core::fmt::{self, Write};

use soma_core::Align;

use crate::style::{ComputedStyle, StyledDoc};
use crate::template::{Declaration, FontSlant, FontWeight, PropertyValue, Selector, Template};
use crate::units::{LengthSpec, LineHeightSpec, PageLength, Rational};
use crate::StyleHash;
use crate::TemplateHash;

pub(crate) const STYLE_SCHEMA: &str = "soma.styles/7";

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

struct FnvWriter {
    state: u64,
}

impl FnvWriter {
    const fn new() -> Self {
        Self { state: FNV_OFFSET }
    }

    const fn finish(self) -> u64 {
        self.state
    }
}

impl Write for FnvWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.as_bytes() {
            self.state ^= *byte as u64;
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        Ok(())
    }
}

pub(crate) fn template_json(template: &Template) -> String {
    let mut out = String::new();
    write_template(&mut out, template).expect("writing into a String cannot fail");
    out
}

pub(crate) fn template_hash(template: &Template) -> TemplateHash {
    let mut writer = FnvWriter::new();
    write_template(&mut writer, template).expect("hashing cannot fail");
    TemplateHash(writer.finish())
}

pub(crate) fn style_json(styled: &StyledDoc) -> String {
    let mut out = String::new();
    write_style(&mut out, styled).expect("writing into a String cannot fail");
    out
}

pub(crate) fn style_hash(styled: &StyledDoc) -> StyleHash {
    let mut writer = FnvWriter::new();
    write_style(&mut writer, styled).expect("hashing cannot fail");
    StyleHash(writer.finish())
}

fn write_template(out: &mut impl Write, template: &Template) -> fmt::Result {
    out.write_str("{\"schema\":\"")?;
    out.write_str(crate::template::SCHEMA)?;
    out.write_str("\",\"name\":")?;
    write_string(out, &template.name)?;
    out.write_str(",\"version\":")?;
    write_opt_string(out, template.version.as_deref())?;

    out.write_str(",\"page\":{\"size\":\"")?;
    out.write_str(template.page.size.as_str())?;
    out.write_str("\",")?;
    let (width, height) = template.page.size.dimensions();
    out.write_str("\"width\":")?;
    write_length_object(out, width)?;
    out.write_str(",\"height\":")?;
    write_length_object(out, height)?;
    out.write_str(",\"marginTop\":")?;
    write_page_length(out, template.page.margin_top)?;
    out.write_str(",\"marginBottom\":")?;
    write_page_length(out, template.page.margin_bottom)?;
    out.write_str(",\"marginLeft\":")?;
    write_page_length(out, template.page.margin_left)?;
    out.write_str(",\"marginRight\":")?;
    write_page_length(out, template.page.margin_right)?;
    out.write_str(",\"header\":")?;
    write_length_object(out, template.page.header)?;
    out.write_str(",\"footer\":")?;
    write_length_object(out, template.page.footer)?;
    out.write_str(",\"headerText\":")?;
    write_opt_string(out, template.page.header_text.as_deref())?;
    out.write_str(",\"folio\":")?;
    write_opt_string(out, template.page.folio.as_deref())?;
    out.write_str(",\"startMaster\":")?;
    write_opt_string(out, template.page.start_master.as_deref())?;
    out.write_str(",\"frontMatter\":\"")?;
    out.write_str(template.page.front_matter.as_str())?;
    out.write_str("\"}")?;

    out.write_str(",\"footnote\":{\"numbering\":\"")?;
    out.write_str(template.footnote.numbering.as_str())?;
    out.write_str("\"}")?;

    out.write_str(",\"toc\":")?;
    match &template.toc {
        Some(toc) => write_toc(out, toc)?,
        None => out.write_str("null")?,
    }

    out.write_str(",\"index\":")?;
    match &template.index {
        Some(index) => {
            out.write_str("{\"title\":")?;
            write_opt_string(out, index.title.as_deref())?;
            // The default order writes no key, so templates that do not ask
            // for pinyin keep the canonical bytes they had before the field
            // existed.
            if index.sort != crate::template::IndexSort::default() {
                out.write_str(",\"sort\":\"")?;
                out.write_str(index.sort.as_str())?;
                out.write_char('\"')?;
            }
            out.write_char('}')?;
        }
        None => out.write_str("null")?,
    }

    out.write_str(",\"cover\":")?;
    match &template.cover {
        Some(cover) => {
            out.write_str("{\"master\":")?;
            write_opt_string(out, cover.master.as_deref())?;
            out.write_char('}')?;
        }
        None => out.write_str("null")?,
    }
    write_numbering(out, &template.numbering)?;

    out.write_str(",\"table\":{\"ruleWidth\":")?;
    write_length_object(out, template.table.rule)?;
    out.write_str(",\"headerRuleWidth\":")?;
    write_length_object(out, template.table.header_rule)?;
    out.write_str(",\"outerRuleWidth\":")?;
    write_length_object(out, template.table.outer_rule)?;
    out.write_str(",\"columnRuleWidth\":")?;
    write_length_object(out, template.table.column_rule)?;
    out.write_str(",\"outerColumnRuleWidth\":")?;
    write_length_object(out, template.table.outer_column_rule)?;
    out.write_str(",\"ruleStyle\":\"")?;
    out.write_str(template.table.style.as_str())?;
    out.write_str("\",\"ruleColor\":")?;
    match template.table.color {
        Some(color) => {
            out.write_char('"')?;
            write_color(out, Some(color))?;
            out.write_char('"')?;
        }
        None => out.write_str("null")?,
    }
    // Keep the key out of the canonical form for the default so templates
    // that do not opt in keep their historical bytes.
    if template.table.width != crate::template::TableWidth::Justify {
        out.write_str(",\"width\":\"")?;
        out.write_str(template.table.width.as_str())?;
        out.write_char('"')?;
    }
    out.write_char('}')?;

    if !template.syntax.is_empty() {
        out.write_str(",\"syntax\":{")?;
        for (index, (class, color)) in template.syntax.entries().enumerate() {
            if index > 0 {
                out.write_char(',')?;
            }
            write!(out, "\"{}\":\"", class.as_str())?;
            write_color(out, Some(color))?;
            out.write_char('"')?;
        }
        out.write_char('}')?;
    }

    out.write_str(",\"masters\":[")?;
    for (index, master) in template.masters.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"name\":")?;
        write_string(out, &master.name)?;
        out.write_str(",\"header\":")?;
        write_length_object(out, master.header)?;
        out.write_str(",\"footer\":")?;
        write_length_object(out, master.footer)?;
        out.write_str(",\"headerText\":")?;
        write_opt_string(out, master.header_text.as_deref())?;
        out.write_str(",\"folio\":")?;
        write_opt_string(out, master.folio.as_deref())?;
        out.write_char('}')?;
    }
    out.write_str("]")?;

    out.write_str(",\"styles\":[")?;
    for (index, rule) in template.styles.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"selector\":")?;
        write_selector(out, &rule.selector)?;
        out.write_str(",\"declarations\":[")?;
        for (declaration_index, declaration) in rule.declarations.iter().enumerate() {
            if declaration_index > 0 {
                out.write_char(',')?;
            }
            write_declaration(out, declaration)?;
        }
        out.write_str("]}")?;
    }
    out.write_str("]}")
}

fn write_selector(out: &mut impl Write, selector: &Selector) -> fmt::Result {
    out.write_str("{\"element\":")?;
    write_opt_string(out, selector.element.as_deref())?;
    out.write_str(",\"role\":")?;
    write_opt_string(out, selector.role.as_deref())?;
    out.write_str(",\"classes\":[")?;
    for (index, class) in selector.classes.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_string(out, class)?;
    }
    out.write_str("]")?;
    out.write_str(",\"within\":")?;
    write_opt_string(out, selector.within.as_deref())?;
    out.write_str(",\"level\":")?;
    match selector.level {
        Some(level) => write!(out, "{level}")?,
        None => out.write_str("null")?,
    }
    // Combinators are omitted when absent so selectors that predate them keep
    // their canonical bytes.
    if let Some(parent) = selector.parent.as_deref() {
        out.write_str(",\"parent\":")?;
        write_string(out, parent)?;
    }
    if !selector.ancestors.is_empty() {
        out.write_str(",\"ancestors\":[")?;
        for (index, ancestor) in selector.ancestors.iter().enumerate() {
            if index > 0 {
                out.write_char(',')?;
            }
            write_string(out, ancestor)?;
        }
        out.write_char(']')?;
    }
    out.write_str("}")
}

fn write_numbering(out: &mut impl Write, spec: &crate::template::NumberingSpec) -> fmt::Result {
    write!(
        out,
        ",\"numbering\":{{\"headings\":\"{}\",\"depth\":{},\"figures\":",
        spec.headings.as_str(),
        spec.depth
    )?;
    write_opt_string(out, spec.figures.as_deref())?;
    out.write_str(",\"tables\":")?;
    write_opt_string(out, spec.tables.as_deref())?;
    out.write_str(",\"equations\":")?;
    write_opt_string(out, spec.equations.as_deref())?;
    out.write_char('}')
}

fn write_declaration(out: &mut impl Write, declaration: &Declaration) -> fmt::Result {
    out.write_str("{\"property\":\"")?;
    out.write_str(declaration.property.as_str())?;
    out.write_str("\",\"value\":")?;
    match &declaration.value {
        PropertyValue::Length(length) => {
            out.write_str("{\"type\":\"length\",")?;
            write_length(out, *length)?;
            out.write_char('}')?;
        }
        PropertyValue::Percent(percent) => {
            out.write_str("{\"type\":\"percent\",")?;
            write_rational(out, percent.value())?;
            out.write_char('}')?;
        }
        PropertyValue::LineHeight(LineHeightSpec::Multiple(value)) => {
            out.write_str("{\"type\":\"multiple\",")?;
            write_rational(out, *value)?;
            out.write_char('}')?;
        }
        PropertyValue::LineHeight(LineHeightSpec::Length(length)) => {
            out.write_str("{\"type\":\"length\",")?;
            write_length(out, *length)?;
            out.write_char('}')?;
        }
        PropertyValue::LineHeight(LineHeightSpec::Percent(percent)) => {
            out.write_str("{\"type\":\"percent\",")?;
            write_rational(out, percent.value())?;
            out.write_char('}')?;
        }
        PropertyValue::Align(align) => {
            out.write_str("{\"type\":\"align\",\"value\":\"")?;
            out.write_str(align_str(*align))?;
            out.write_str("\"}")?;
        }
        PropertyValue::Bool(flag) => write!(out, "{{\"type\":\"bool\",\"value\":{flag}}}")?,
        PropertyValue::Count(count) => write!(out, "{{\"type\":\"count\",\"value\":{count}}}")?,
        PropertyValue::Weight(weight) => {
            out.write_str("{\"type\":\"weight\",\"value\":\"")?;
            out.write_str(weight_str(*weight))?;
            out.write_str("\"}")?;
        }
        PropertyValue::Slant(slant) => {
            out.write_str("{\"type\":\"slant\",\"value\":\"")?;
            out.write_str(slant_str(*slant))?;
            out.write_str("\"}")?;
        }
        PropertyValue::Color(color) => {
            out.write_str("{\"type\":\"color\",\"value\":\"")?;
            write_color(out, Some(*color))?;
            out.write_str("\"}")?;
        }
        PropertyValue::Text(text) => {
            out.write_str("{\"type\":\"text\",\"value\":")?;
            write_string(out, text)?;
            out.write_char('}')?;
        }
    }
    out.write_char('}')
}

fn write_length(out: &mut impl Write, length: LengthSpec) -> fmt::Result {
    write_rational(out, length.value)?;
    out.write_str(",\"unit\":\"")?;
    out.write_str(length.unit.as_str())?;
    out.write_char('"')
}

/// A length as a complete JSON object (page geometry and table rules).
fn write_length_object(out: &mut impl Write, length: LengthSpec) -> fmt::Result {
    out.write_char('{')?;
    write_length(out, length)?;
    out.write_char('}')
}

/// A page margin: lengths keep the historical `{num, den, unit}` object;
/// percentages add `"unit":"%"` and carry the fraction as `num/den`.
fn write_page_length(out: &mut impl Write, length: PageLength) -> fmt::Result {
    match length {
        PageLength::Length(length) => write_length_object(out, length),
        PageLength::Percent(percent) => {
            out.write_char('{')?;
            write_rational(out, percent.value())?;
            out.write_str(",\"unit\":\"%\"}")
        }
    }
}

fn write_rational(out: &mut impl Write, value: Rational) -> fmt::Result {
    write!(out, "\"num\":{},\"den\":{}", value.num(), value.den())
}

fn write_style(out: &mut impl Write, styled: &StyledDoc) -> fmt::Result {
    out.write_str("{\"schema\":\"")?;
    out.write_str(STYLE_SCHEMA)?;
    write!(out, "\",\"templateHash\":{}", styled.template_hash().0)?;
    out.write_str(",\"bodyPt\":")?;
    write_rational(out, styled.body_pt())?;

    let page = styled.page();
    write!(out, ",\"page\":{{\"width\":{}", page.width.0)?;
    write!(out, ",\"height\":{}", page.height.0)?;
    write!(out, ",\"marginTop\":{}", page.margin_top.0)?;
    write!(out, ",\"marginBottom\":{}", page.margin_bottom.0)?;
    write!(out, ",\"marginLeft\":{}", page.margin_left.0)?;
    write!(out, ",\"marginRight\":{}}}", page.margin_right.0)?;
    write!(out, ",\"headerHeight\":{}", page.header_height.0)?;
    write!(out, ",\"footerHeight\":{}", page.footer_height.0)?;

    out.write_str(",\"headerText\":")?;
    write_opt_string(out, styled.header_text())?;
    out.write_str(",\"folio\":")?;
    write_opt_string(out, styled.folio())?;
    write!(out, ",\"startMaster\":")?;
    match styled.start_master() {
        Some(index) => write!(out, "{index}")?,
        None => out.write_str("null")?,
    }
    out.write_str(",\"frontMatter\":\"")?;
    out.write_str(styled.front_matter().as_str())?;
    out.write_char('"')?;

    out.write_str(",\"footnote\":{\"numbering\":\"")?;
    out.write_str(styled.footnote_numbering().as_str())?;
    out.write_str("\"}")?;

    out.write_str(",\"toc\":")?;
    match styled.toc() {
        Some(toc) => write_toc(out, toc)?,
        None => out.write_str("null")?,
    }

    out.write_str(",\"index\":")?;
    match styled.index() {
        Some(index) => {
            out.write_str("{\"title\":")?;
            write_opt_string(out, index.title.as_deref())?;
            out.write_char('}')?;
        }
        None => out.write_str("null")?,
    }

    out.write_str(",\"cover\":")?;
    match styled.cover() {
        Some(cover) => {
            out.write_str("{\"master\":")?;
            write_opt_string(out, cover.master.as_deref())?;
            out.write_char('}')?;
        }
        None => out.write_str("null")?,
    }
    write_numbering(out, styled.numbering())?;

    out.write_str(",\"masters\":[")?;
    for (index, master) in styled.masters().iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"name\":")?;
        write_string(out, &master.name)?;
        write!(out, ",\"headerHeight\":{}", master.header_height.0)?;
        write!(out, ",\"footerHeight\":{}", master.footer_height.0)?;
        out.write_str(",\"headerText\":")?;
        write_opt_string(out, master.header_text.as_deref())?;
        out.write_str(",\"folio\":")?;
        write_opt_string(out, master.folio.as_deref())?;
        out.write_char('}')?;
    }
    out.write_str("]")?;

    if !styled.table().syntax().is_empty() {
        out.write_str(",\"syntax\":{")?;
        for (index, (class, color)) in styled.table().syntax().entries().enumerate() {
            if index > 0 {
                out.write_char(',')?;
            }
            write!(out, "\"{}\":\"", class.as_str())?;
            write_color(out, Some(color))?;
            out.write_char('"')?;
        }
        out.write_char('}')?;
    }

    out.write_str(",\"styles\":[")?;
    for (index, style) in styled.table().styles().iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_computed_style(out, style)?;
    }
    out.write_str("]")?;

    out.write_str(",\"nodeStyle\":[")?;
    for (index, (node, style)) in styled.node_style().iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(out, "{{\"node\":{},\"style\":{}}}", node.0, style.0)?;
    }
    out.write_str("]}")
}

fn write_computed_style(out: &mut impl Write, style: &ComputedStyle) -> fmt::Result {
    write!(out, "{{\"fontSize\":{}", style.font_size.0)?;
    write!(out, ",\"lineHeight\":{}", style.line_height.0)?;
    write!(out, ",\"firstIndent\":{}", style.first_indent.0)?;
    write!(out, ",\"indentLeft\":{}", style.indent_left.0)?;
    write!(out, ",\"indentRight\":{}", style.indent_right.0)?;
    write!(out, ",\"spaceBefore\":{}", style.space_before.0)?;
    write!(out, ",\"spaceAfter\":{}", style.space_after.0)?;
    write!(out, ",\"spaceBeforePlus\":{}", style.space_before_plus.0)?;
    write!(out, ",\"spaceBeforeMinus\":{}", style.space_before_minus.0)?;
    write!(out, ",\"spaceAfterPlus\":{}", style.space_after_plus.0)?;
    write!(out, ",\"spaceAfterMinus\":{}", style.space_after_minus.0)?;
    write!(out, ",\"widowLines\":{}", style.widow_lines)?;
    write!(out, ",\"orphanLines\":{}", style.orphan_lines)?;
    write!(out, ",\"hangingPunctuation\":{}", style.hanging_punctuation)?;
    write!(out, ",\"hangingCap\":{}", style.hanging_cap.0)?;
    write!(out, ",\"baselineGrid\":{}", style.baseline_grid.0)?;
    out.write_str(",\"align\":\"")?;
    out.write_str(align_str(style.align))?;
    out.write_str("\",\"family\":")?;
    write_opt_string(out, style.family.as_deref())?;
    out.write_str(",\"color\":")?;
    match style.color {
        Some(color) => {
            out.write_char('"')?;
            write_color(out, Some(color))?;
            out.write_char('"')?;
        }
        None => out.write_str("null")?,
    }
    out.write_str(",\"weight\":\"")?;
    out.write_str(weight_str(style.weight))?;
    out.write_str("\",\"slant\":\"")?;
    out.write_str(slant_str(style.slant))?;
    write!(out, "\",\"underline\":{}", style.underline)?;
    write!(out, ",\"smallCaps\":{}", style.small_caps)?;
    write!(out, ",\"keepWithNext\":{}", style.keep_with_next)?;
    write!(out, ",\"pageBreakBefore\":{}", style.page_break_before)?;
    if let Some(background) = style.background {
        out.write_str(",\"background\":\"")?;
        write_color(out, Some(background))?;
        out.write_char('"')?;
    }
    if style.padding_top.0 != 0 {
        write!(out, ",\"paddingTop\":{}", style.padding_top.0)?;
    }
    if style.padding_right.0 != 0 {
        write!(out, ",\"paddingRight\":{}", style.padding_right.0)?;
    }
    if style.padding_bottom.0 != 0 {
        write!(out, ",\"paddingBottom\":{}", style.padding_bottom.0)?;
    }
    if style.padding_left.0 != 0 {
        write!(out, ",\"paddingLeft\":{}", style.padding_left.0)?;
    }
    out.write_char('}')
}

fn write_color(out: &mut impl Write, color: Option<u32>) -> fmt::Result {
    let Some(color) = color else {
        return Ok(());
    };
    write!(
        out,
        "#{:02x}{:02x}{:02x}",
        color >> 16,
        (color >> 8) & 0xff,
        color & 0xff
    )
}

fn align_str(align: Align) -> &'static str {
    match align {
        Align::Justify => "justify",
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn weight_str(weight: FontWeight) -> &'static str {
    match weight {
        FontWeight::Normal => "normal",
        FontWeight::Bold => "bold",
    }
}

fn slant_str(slant: FontSlant) -> &'static str {
    match slant {
        FontSlant::Normal => "normal",
        FontSlant::Italic => "italic",
    }
}

fn write_opt_string(out: &mut impl Write, value: Option<&str>) -> fmt::Result {
    match value {
        Some(value) => write_string(out, value),
        None => out.write_str("null"),
    }
}

fn write_toc(out: &mut impl Write, toc: &crate::template::TocSpec) -> fmt::Result {
    write!(out, "{{\"depth\":{},\"title\":", toc.depth)?;
    write_opt_string(out, toc.title.as_deref())?;
    write!(out, ",\"leaders\":{},\"figures\":", toc.leaders)?;
    write_opt_string(out, toc.figures.as_deref())?;
    out.write_str(",\"tables\":")?;
    write_opt_string(out, toc.tables.as_deref())?;
    out.write_char('}')
}

fn write_string(out: &mut impl Write, value: &str) -> fmt::Result {
    out.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            character if (character as u32) < 0x20 => {
                write!(out, "\\u{:04x}", character as u32)?;
            }
            character => out.write_char(character)?,
        }
    }
    out.write_char('"')
}
