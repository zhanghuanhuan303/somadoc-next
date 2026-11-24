//! A2 template parsing: YAML subset behaviour, validation, hashing.

use soma_template::{
    parse_template, FootnoteNumbering, FrontMatterNumbering, HeadingNumbering, LineHeightSpec,
    PageSize, Property, PropertyValue, Rational, Unit,
};

const FULL: &str = r##"
# a book template
name: book
version: "1"
page:
  size: A4
  margin: 20mm
footnote:
  numbering: page
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
      family: serif
      color: 101010
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
      space-after: 0
  - selector:
      element: strong
    declarations:
      weight: bold
  - selector:
      element: heading
      class: chapter
    declarations:
      font-size: 20pt
      space-before: 1.5em
      keep-with-next: true
"##;

#[test]
fn parses_a_full_template() {
    let template = parse_template(FULL).expect("fixture must parse");

    assert_eq!(template.name, "book");
    assert_eq!(template.version.as_deref(), Some("1"));
    assert_eq!(template.page.size, PageSize::A4);
    assert_eq!(template.footnote.numbering, FootnoteNumbering::Page);
    assert_eq!(template.styles.len(), 4);

    // 10.5pt becomes the exact rational 21/2.
    assert_eq!(template.body_font_pt().num(), 21);
    assert_eq!(template.body_font_pt().den(), 2);

    let paragraph = &template.styles[1];
    assert_eq!(paragraph.selector.element.as_deref(), Some("paragraph"));
    match &paragraph.declarations[0].value {
        soma_template::PropertyValue::Length(length) => {
            assert_eq!(length.unit, Unit::Em);
            assert_eq!(length.value.num(), 2);
            assert_eq!(length.value.den(), 1);
        }
        other => panic!("expected a length, got {other:?}"),
    }

    let heading = &template.styles[3];
    assert_eq!(heading.selector.classes, vec!["chapter".to_string()]);
    assert!(heading
        .declarations
        .iter()
        .any(|declaration| declaration.property == Property::KeepWithNext));
}

#[test]
fn canonical_json_is_stable_and_namespaced() {
    let template = parse_template(FULL).expect("fixture must parse");
    let json = template.canonical_json();

    assert!(
        json.starts_with("{\"schema\":\"soma.template/9\""),
        "{json}"
    );
    assert!(json.contains("\"size\":\"A4\""));
    assert!(json.contains("\"unit\":\"pt\""));
    assert!(json.contains("\"numbering\":\"page\""));
    assert_eq!(json, template.canonical_json());
    assert_eq!(json, parse_template(FULL).unwrap().canonical_json());
}

#[test]
fn template_hash_is_sensitive_to_the_declarations() {
    let first = parse_template(FULL).expect("fixture must parse");
    let second = parse_template(&FULL.replace("2em", "2.5em")).expect("edited fixture must parse");

    assert_eq!(
        first.template_hash(),
        parse_template(FULL).unwrap().template_hash()
    );
    assert_ne!(first.template_hash(), second.template_hash());
}

#[test]
fn unknown_property_is_rejected_with_a_position() {
    let source = FULL.replace("first-indent: 2em", "first-indnet: 2em");
    let error = parse_template(&source).expect_err("typo must fail");

    assert_eq!(error.reason, "unknown style property");
    assert!(
        error.line > 1,
        "line must point at the declaration: {error}"
    );
    assert!(error.path.starts_with("styles[1]"), "path: {error}");
}

#[test]
fn unknown_key_and_element_are_rejected() {
    let error = parse_template(&FULL.replace("version: \"1\"", "verison: \"1\""))
        .expect_err("typo must fail");
    assert_eq!(error.reason, "unknown key");

    let error = parse_template(&FULL.replace("element: paragraph", "element: paragraf"))
        .expect_err("typo must fail");
    assert_eq!(error.reason, "unknown element selector");
}

#[test]
fn unknown_page_size_is_rejected() {
    let error = parse_template(&FULL.replace("size: A4", "size: B5")).expect_err("size must fail");
    assert_eq!(error.reason, "page size must be A4, A5, Letter, or custom");
}

#[test]
fn duplicate_keys_are_rejected() {
    let source = FULL.replace("name: book", "name: book\nname: other");
    let error = parse_template(&source).expect_err("duplicate must fail");
    assert_eq!(error.reason, "duplicate key");
}

#[test]
fn the_document_font_size_is_required_and_absolute() {
    let missing = FULL.replace("      font-size: 10.5pt\n", "");
    let error = parse_template(&missing).expect_err("font-size is required");
    assert_eq!(
        error.reason,
        "template must set font-size on the document element"
    );

    let relative = FULL.replace("font-size: 10.5pt", "font-size: 1.2em");
    let error = parse_template(&relative).expect_err("relative root font is invalid");
    assert_eq!(error.reason, "document font-size must use an absolute unit");
}

#[test]
fn invalid_values_are_rejected() {
    let error =
        parse_template(&FULL.replace("line-height: 1.6", "line-height: tall")).expect_err("bad");
    assert_eq!(error.reason, "invalid line-height value");

    let error = parse_template(&FULL.replace("first-indent: 2em", "first-indent: 2"))
        .expect_err("bare non-zero number is ambiguous");
    assert_eq!(error.reason, "invalid length value");

    let error = parse_template(&FULL.replace("color: 101010", "color: red")).expect_err("bad");
    assert_eq!(error.reason, "color must be #rrggbb");

    let error = parse_template(&FULL.replace("keep-with-next: true", "keep-with-next: maybe"))
        .expect_err("bad");
    assert_eq!(error.reason, "expected true or false");
}

#[test]
fn comments_and_quoted_scalars_are_handled() {
    let source = r#"
name: 'quo''ted'   # trailing comment
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      family: "Noto Serif CJK SC"
"#;
    let template = parse_template(source).expect("fixture must parse");
    assert_eq!(template.name, "quo'ted");
    assert!(template
        .styles
        .iter()
        .flat_map(|rule| &rule.declarations)
        .any(|declaration| matches!(
            &declaration.value,
            soma_template::PropertyValue::Text(text) if text == "Noto Serif CJK SC"
        )));
}

#[test]
fn a_within_selector_parses_and_canonicalizes() {
    let source = r#"
name: cells
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: paragraph
      within: cell
    declarations:
      first-indent: 0
      indent-left: 0.25em
"#;
    let template = parse_template(source).expect("within must parse");
    let rule = template
        .styles
        .iter()
        .find(|rule| rule.selector.within.as_deref() == Some("cell"))
        .expect("cell-context rule");
    assert_eq!(rule.selector.element.as_deref(), Some("paragraph"));

    let json = template.canonical_json();
    assert!(json.contains("\"within\":\"cell\""), "{json}");
    assert_eq!(json, template.canonical_json(), "deterministic");

    let error = parse_template(&source.replace("within: cell", "within: celll")).expect_err("bad");
    assert_eq!(error.reason, "unknown within selector");
    assert!(
        !rule.selector.matches("paragraph", None, &[], &[], None),
        "within: cell must not match outside a cell"
    );
    assert!(
        rule.selector
            .matches("paragraph", None, &[], &["table", "row", "cell"], None),
        "within: cell matches inside a cell"
    );
}

#[test]
fn a_table_block_parses_and_canonicalizes() {
    let source = r#"
name: ruled
page:
  size: A4
table:
  rule-width: 0.5pt
  header-rule-width: 0.75pt
  outer-rule-width: 1pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("table section must parse");
    assert_eq!(
        template.table.rule,
        soma_template::LengthSpec::parse("0.5pt").unwrap()
    );
    assert_eq!(
        template.table.header_rule,
        soma_template::LengthSpec::parse("0.75pt").unwrap()
    );
    assert_eq!(
        template.table.outer_rule,
        soma_template::LengthSpec::parse("1pt").unwrap()
    );
    assert!(!template.table.is_zero());

    let json = template.canonical_json();
    assert!(
        json.contains("\"table\":{\"ruleWidth\":{\"num\":1,\"den\":2,\"unit\":\"pt\"}"),
        "{json}"
    );

    let error = parse_template(&source.replace("rule-width: 0.5pt", "rule-width: -0.5pt"))
        .expect_err("negative rules are invalid");
    assert_eq!(error.reason, "table rule width must not be negative");

    let error = parse_template(&source.replace("outer-rule-width: 1pt", "outer: 1pt"))
        .expect_err("unknown table key");
    assert!(error.reason.contains("unknown"));
}

#[test]
fn column_rules_style_and_color_parse() {
    let source = r##"
name: grid
page:
  size: A4
table:
  column-rule-width: 0.4pt
  outer-column-rule-width: 0.8pt
  rule-style: dashed
  rule-color: "#aa0000"
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"##;
    let template = parse_template(source).expect("column rules parse");
    assert_eq!(
        template.table.column_rule,
        soma_template::LengthSpec::parse("0.4pt").unwrap()
    );
    assert_eq!(template.table.style, soma_template::RuleStyle::Dashed);
    assert_eq!(template.table.color, Some(0xaa0000));

    let json = template.canonical_json();
    assert!(json.contains("\"ruleStyle\":\"dashed\""), "{json}");
    assert!(json.contains("\"ruleColor\":\"#aa0000\""), "{json}");

    let error = parse_template(&source.replace("dashed", "dotted")).expect_err("bad style");
    assert_eq!(error.reason, "rule style must be solid or dashed");

    let error = parse_template(&source.replace("aa0000", "red")).expect_err("bad color");
    assert_eq!(error.reason, "rule color must be #rrggbb");
}

#[test]
fn masters_parse_canonicalize_and_reject_bad_names() {
    let source = r#"
name: masters
page:
  size: A4
  header: 6mm
  footer: 6mm
masters:
  - name: chapter
    header: 4mm
    footer: 0
    header-text: 章节页
  - name: cover
    folio: null
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("masters parse");
    assert_eq!(template.masters.len(), 2);
    assert_eq!(template.masters[0].name, "chapter");
    assert_eq!(template.masters[0].header_text.as_deref(), Some("章节页"));
    assert_eq!(template.masters[1].name, "cover");

    let json = template.canonical_json();
    assert!(
        json.contains("\"masters\":[{\"name\":\"chapter\""),
        "{json}"
    );
    assert_eq!(json, template.canonical_json());

    let error =
        parse_template(&source.replace("name: cover", "name: chapter")).expect_err("duplicate");
    assert_eq!(error.reason, "duplicate or reserved master name");

    let error =
        parse_template(&source.replace("name: cover", "name: default")).expect_err("reserved");
    assert_eq!(error.reason, "duplicate or reserved master name");

    let error =
        parse_template(&source.replace("header: 4mm", "header: -4mm")).expect_err("negative");
    assert_eq!(error.reason, "master band height must not be negative");

    let error =
        parse_template(&source.replace("header-text:", "headerText:")).expect_err("unknown key");
    assert!(error.reason.contains("unknown"));
}

#[test]
fn a_toc_section_parses_with_a_depth_and_title() {
    let source = r#"
name: toc
page:
  size: A4
  margin: 20mm
toc:
  depth: 3
  title: 目次
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("toc parses");
    let toc = template.toc.as_ref().expect("toc");
    assert_eq!(toc.depth, 3);
    assert_eq!(toc.title.as_deref(), Some("目次"));
    assert!(toc.leaders, "leaders default to on");
    assert!(toc.figures.is_none());

    let json = template.canonical_json();
    assert!(
        json.contains(
            "\"toc\":{\"depth\":3,\"title\":\"目次\",\"leaders\":true,\"figures\":null,\"tables\":null}"
        ),
        "{json}"
    );
    assert_eq!(json, template.canonical_json());

    // Depth defaults to 2; an empty value means no heading.
    let defaulted = parse_template(&source.replace("  depth: 3\n", "")).expect("default depth");
    assert_eq!(defaulted.toc.as_ref().unwrap().depth, 2);
    let untitled =
        parse_template(&source.replace("  title: 目次", "  title:")).expect("null title");
    assert!(untitled.toc.unwrap().title.is_none());

    let error = parse_template(&source.replace("depth: 3", "depth: 4")).expect_err("too deep");
    assert!(error.reason.contains("toc depth"));
    let error = parse_template(&source.replace("depth: 3", "depth: x")).expect_err("not a number");
    assert!(error.reason.contains("toc depth"));
    let error = parse_template(&source.replace("depth: 3", "entries: 3")).expect_err("unknown key");
    assert!(error.reason.contains("unknown"));
}

#[test]
fn a_toc_can_print_figure_and_table_lists_and_drop_leaders() {
    let source = r#"
name: toc
page:
  size: A4
  margin: 20mm
toc:
  depth: 2
  leaders: false
  figures: 图目录
  tables: 表目录
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("toc parses");
    let toc = template.toc.as_ref().expect("toc");

    assert!(!toc.leaders);
    assert_eq!(toc.figures.as_deref(), Some("图目录"));
    assert_eq!(toc.tables.as_deref(), Some("表目录"));

    // A quoted empty title still emits the list, just without a heading.
    let untitled = parse_template(&source.replace("figures: 图目录", "figures: \"\""))
        .expect("empty list title");
    assert_eq!(untitled.toc.unwrap().figures.as_deref(), Some(""));

    let error =
        parse_template(&source.replace("leaders: false", "leaders: maybe")).expect_err("bad flag");
    assert!(error.reason.contains("leaders"));
}

#[test]
fn numbering_and_front_matter_parse_and_round_trip() {
    let source = r#"
name: numbered
page:
  size: A4
  margin: 20mm
  front-matter: roman
numbering:
  headings: chinese-chapter
  depth: 2
  figures: "图 {n}"
  tables: "表 {n}"
  equations: "({n})"
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("numbering parses");

    assert_eq!(
        template.numbering.headings,
        HeadingNumbering::ChineseChapter
    );
    assert_eq!(template.numbering.depth, 2);
    assert_eq!(template.numbering.figures.as_deref(), Some("图 {n}"));
    assert_eq!(template.numbering.equations.as_deref(), Some("({n})"));
    assert_eq!(template.page.front_matter, FrontMatterNumbering::Roman);
    let json = template.canonical_json();
    assert!(
        json.contains(
            "\"numbering\":{\"headings\":\"chinese-chapter\",\"depth\":2,\"figures\":\"图 {n}\",\"tables\":\"表 {n}\",\"equations\":\"({n})\"}"
        ),
        "{json}"
    );
    assert!(json.contains("\"frontMatter\":\"roman\""), "{json}");

    let error =
        parse_template(&source.replace("图 {n}", "图")).expect_err("a label needs the placeholder");
    assert!(error.reason.contains("{n}"));
    let error =
        parse_template(&source.replace("chinese-chapter", "roman")).expect_err("unknown numbering");
    assert!(error.reason.contains("numbering.headings"));
    let error = parse_template(&source.replace("front-matter: roman", "front-matter: greek"))
        .expect_err("unknown front matter");
    assert!(error.reason.contains("front-matter"));
    let error = parse_template(&source.replace("depth: 2", "depth: 5"))
        .expect_err("numbering depth is bounded");
    assert!(error.reason.contains("numbering.depth"));
}

#[test]
fn a_page_master_can_be_bound_to_section_starts() {
    let source = r#"
name: chapter-start
page:
  size: A4
  margin: 20mm
  header: 6mm
  start-master: plain
masters:
  - name: plain
    header: 0
    footer: 0
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("start master parses");
    assert_eq!(template.page.start_master.as_deref(), Some("plain"));
    let json = template.canonical_json();
    assert!(json.contains("\"startMaster\":\"plain\""), "{json}");
}

#[test]
fn a_selector_can_match_a_heading_level() {
    let source = r#"
name: levels
page:
  size: A4
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
  - selector:
      element: heading
      level: 1
    declarations:
      font-size: 20pt
  - selector:
      element: heading
      level: 2
    declarations:
      font-size: 14pt
"#;
    let template = parse_template(source).expect("levels parse");
    assert_eq!(template.styles[1].selector.level, Some(1));
    assert_eq!(template.styles[2].selector.level, Some(2));
    let json = template.canonical_json();
    assert!(
        json.contains(
            "\"element\":\"heading\",\"role\":null,\"classes\":[],\"within\":null,\"level\":1"
        ),
        "{json}"
    );

    let error = parse_template(&source.replace("level: 2", "level: 0")).expect_err("zero level");
    assert!(error.reason.contains("level must be"));
    let error = parse_template(&source.replace("level: 2", "level: x")).expect_err("not a number");
    assert!(error.reason.contains("level must be"));
    // A selector may be level-only.
    let level_only = source.replace("      element: heading\n      level: 2", "      level: 2");
    let template = parse_template(&level_only).expect("level-only selector");
    assert!(template.styles[2].selector.element.is_none());
}

#[test]
fn an_index_section_parses_and_round_trips() {
    let source = r#"
name: indexed
page:
  size: A4
  margin: 20mm
index:
  title: 索引
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
"#;
    let template = parse_template(source).expect("index parses");
    assert_eq!(
        template
            .index
            .as_ref()
            .and_then(|index| index.title.as_deref()),
        Some("索引")
    );
    let json = template.canonical_json();
    assert!(json.contains("\"index\":{\"title\":\"索引\"}"), "{json}");

    let untitled = parse_template(&source.replace("  title: 索引\n", "")).expect("empty index");
    assert!(
        untitled.index.is_some(),
        "an empty section still asks for an index"
    );
    assert!(untitled.index.unwrap().title.is_none());

    let absent = parse_template(&source.replace("index:\n  title: 索引\n", "")).expect("no index");
    assert!(absent.index.is_none());

    let error =
        parse_template(&source.replace("title: 索引", "entries: 3")).expect_err("unknown key");
    assert!(error.reason.contains("unknown"));

    // The order is a template decision; the default writes no canonical key so
    // existing templates keep their bytes.
    let pinyin = source.replace("  title: 索引\n", "  title: 索引\n  sort: pinyin\n");
    let template = parse_template(&pinyin).expect("sort parses");
    assert_eq!(
        template.index.as_ref().map(|index| index.sort),
        Some(soma_template::IndexSort::Pinyin)
    );
    let json = template.canonical_json();
    assert!(
        json.contains("\"index\":{\"title\":\"索引\",\"sort\":\"pinyin\"}"),
        "{json}"
    );
    let bad = source.replace("  title: 索引\n", "  title: 索引\n  sort: stroke\n");
    let error = parse_template(&bad).expect_err("unknown order");
    assert!(error.reason.contains("codepoint or pinyin"));
}

/// The reference page design transcribed from pandoc's `soma.latex`.
///
/// Keeping this as a test means an edit to the example template cannot silently
/// drift from the design record: one-inch margins, Plex-sized 10/14.4pt text,
/// no first-line indent, a 20pt paragraph skip, italic quotations, and the
/// booktabs three-rule table.
#[test]
fn plex_reference_template_matches_the_pandoc_design() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/templates/plex.yaml"
    ));
    let template = parse_template(source).expect("plex reference template must parse");

    assert_eq!(template.name, "soma-plex");
    assert_eq!(template.page.size, PageSize::A4);
    let margin = Rational::new(254, 10).expect("non-zero denominator");
    assert_eq!(template.page.margin_top.value, margin);
    assert_eq!(template.page.margin_top.unit, Unit::Mm);
    assert_eq!(template.page.margin_bottom.value, margin);
    assert_eq!(template.page.folio.as_deref(), Some("{page}"));
    assert!(template.cover.is_some());
    assert_eq!(template.toc.as_ref().map(|toc| toc.depth), Some(2));
    assert_eq!(
        template.index.as_ref().map(|index| index.sort),
        Some(soma_template::IndexSort::Pinyin)
    );
    assert_eq!(template.table.width, soma_template::TableWidth::Natural);
    assert!(
        template.canonical_json().contains("\"width\":\"natural\""),
        "a natural table is part of the template hash"
    );

    let declaration = |element: &str, level: Option<u8>, property: Property| {
        template
            .styles
            .iter()
            .find(|rule| {
                rule.selector.element.as_deref() == Some(element) && rule.selector.level == level
            })
            .and_then(|rule| {
                rule.declarations
                    .iter()
                    .find(|declaration| declaration.property == property)
            })
            .map(|declaration| &declaration.value)
    };

    assert_eq!(
        declaration("document", None, Property::Family),
        Some(&PropertyValue::Text("sans".to_string()))
    );
    assert_eq!(
        declaration("document", None, Property::LineHeight),
        Some(&PropertyValue::LineHeight(LineHeightSpec::Multiple(
            Rational::new(36, 25).expect("1.44")
        )))
    );
    assert_eq!(
        declaration("paragraph", None, Property::FirstIndent),
        Some(&PropertyValue::Length(soma_template::LengthSpec::zero()))
    );
    let twenty_points = Rational::new(20, 1).expect("20pt");
    assert_eq!(
        declaration("paragraph", None, Property::SpaceAfter),
        Some(&PropertyValue::Length(soma_template::LengthSpec::new(
            twenty_points,
            Unit::Pt
        )))
    );
    assert_eq!(
        declaration("blockQuote", None, Property::Slant),
        Some(&PropertyValue::Slant(soma_template::FontSlant::Italic))
    );
    assert_eq!(
        declaration("heading", Some(1), Property::FontSize),
        Some(&PropertyValue::Length(soma_template::LengthSpec::new(
            Rational::new(72, 5).expect("14.4pt"),
            Unit::Pt
        )))
    );

    let header_rule = Rational::new(6, 10).expect("0.6pt");
    assert_eq!(template.table.header_rule.value, header_rule);
    assert_eq!(template.table.header_rule.unit, Unit::Pt);
    assert_eq!(template.table.rule.value, Rational::ZERO);
    assert_eq!(template.table.column_rule.value, Rational::ZERO);

    // ADR-0090: the code panel is a template declaration, not a renderer
    // constant.
    assert_eq!(
        declaration("codeBlock", None, Property::Background),
        Some(&PropertyValue::Color(0xf6f7f6))
    );
    let pad = Rational::new(1, 2).expect("0.5em");
    let pad = soma_template::LengthSpec::new(pad, Unit::Em);
    assert_eq!(
        declaration("codeBlock", None, Property::PaddingTop),
        Some(&PropertyValue::Length(pad))
    );
    assert_eq!(
        declaration("codeBlock", None, Property::PaddingBottom),
        Some(&PropertyValue::Length(pad))
    );

    // ADR-0091: mdBook's Rust palette.
    use soma_core::SyntaxClass;
    assert_eq!(template.syntax.color(SyntaxClass::Comment), Some(0x575757));
    assert_eq!(template.syntax.color(SyntaxClass::Keyword), Some(0x9d00ec));
    assert_eq!(template.syntax.color(SyntaxClass::String), Some(0x008200));
    assert_eq!(template.syntax.color(SyntaxClass::Function), Some(0x0030f2));
}

#[test]
fn negative_padding_is_rejected() {
    let source = r##"
name: bad
page:
  size: A4
  margin: 20mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
  - selector:
      element: codeBlock
    declarations:
      padding-left: -1em
"##;
    let error = parse_template(source).expect_err("negative padding must fail");
    assert!(error.to_string().contains("padding"));
}
