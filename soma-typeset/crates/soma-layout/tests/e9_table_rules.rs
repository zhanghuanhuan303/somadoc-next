//! Table rules: outer, header and internal lines.

mod common;

use common::{document, engine, metrics, styled, text, TIGHT};
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign};
use soma_layout::{layout, BoxKind, PagedLayout, PlacedBlock, TextSource};

const RULED: &str = r#"
name: ruled
page:
  size: custom
  width: 100mm
  height: 34mm
  margin: 4mm
table:
  rule-width: 0.4pt
  header-rule-width: 0.6pt
  outer-rule-width: 0.8pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
"#;

/// 0.8pt, 0.6pt and 0.4pt at a 10.5pt body, in 1/4096 em.
const OUTER: i32 = 312;
const HEADER_RULE: i32 = 234;
const RULE: i32 = 156;
/// One 10.5pt line at line-height 1.6.
const LINE: i32 = 6553;

fn cell(value: &str, colspan: u16, rowspan: u16) -> Cell {
    let paragraph = Paragraph::new(NodeMeta::synthesized(), vec![text(value)]);
    Cell::new(NodeMeta::synthesized(), vec![Block::Paragraph(paragraph)])
        .with_spans(colspan, rowspan)
}

fn columns(count: usize) -> Vec<Column> {
    (0..count)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect()
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

fn of_kind(layout: &PagedLayout, page: usize, kind: BoxKind) -> Vec<&PlacedBlock> {
    layout.pages[page]
        .blocks
        .iter()
        .filter(|block| block.kind == kind)
        .collect()
}

#[test]
fn rules_bracket_every_row_and_repeat_on_continuation_pages() {
    let mut rows = vec![Row::new(
        NodeMeta::synthesized(),
        true,
        vec![cell("表头", 1, 1)],
    )];
    for index in 0..30 {
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&format!("行{index:02}"), 1, 1)],
        ));
    }
    let table = Table::new(NodeMeta::synthesized(), columns(1), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, RULED);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 3, "the table must continue");
    let content_width = layout.page.content_width().0;

    for page in 0..layout.pages.len() {
        let rules = of_kind(&layout, page, BoxKind::TableRule);
        let cells = of_kind(&layout, page, BoxKind::TableCell);
        assert!(!cells.is_empty(), "page {page} has cells");

        // The repeated header: outer rule, header cell, header rule.
        let top = rules[0];
        assert_eq!(top.y.0, 0, "page {page} starts with the outer rule");
        assert_eq!(top.height.0, OUTER);
        assert_eq!(top.x.0, 0);
        assert_eq!(top.width.0, content_width);

        let header = cells
            .iter()
            .find(|block| block_text(&layout, block) == "表头")
            .unwrap_or_else(|| panic!("page {page} repeats the header"));
        assert_eq!(header.y.0, OUTER, "header sits below the outer rule");

        let header_rule = rules[1];
        assert_eq!(header_rule.y.0, OUTER + LINE);
        assert_eq!(header_rule.height.0, HEADER_RULE);
        assert_eq!(header_rule.width.0, content_width);

        let body: Vec<_> = cells
            .iter()
            .filter(|block| block_text(&layout, block) != "表头")
            .collect();
        assert!(!body.is_empty(), "page {page} has body rows");
        assert!(
            body[0].y.0 >= OUTER + LINE + HEADER_RULE,
            "body starts below the header rule"
        );

        // Each body row carries its own trailing rule; the last page ends
        // with the outer bottom rule instead of an internal one.
        assert_eq!(
            rules.len(),
            2 + body.len(),
            "page {page} has one rule per body row"
        );
        let last_rule = rules.last().unwrap();
        assert_eq!(
            last_rule.height.0,
            if page + 1 == layout.pages.len() {
                OUTER
            } else {
                RULE
            }
        );
    }

    assert!(layout.pages.iter().all(|page| !page.overflow));
    assert!(layout
        .pages
        .iter()
        .all(|page| page.used_height.0 <= layout.page.content_height().0));
}

#[test]
fn an_internal_rule_skips_a_spanning_cell() {
    let rows = vec![
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("左上", 1, 1), cell("右上", 1, 1)],
        ),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("跨两行", 1, 2), cell("右一", 1, 1)],
        ),
        Row::new(NodeMeta::synthesized(), false, vec![cell("右二", 1, 1)]),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("左下", 1, 1), cell("右下", 1, 1)],
        ),
    ];
    let table = Table::new(NodeMeta::synthesized(), columns(2), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, RULED);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 1, "the fixture fits one page");
    let content_width = layout.page.content_width().0;
    let rules = of_kind(&layout, 0, BoxKind::TableRule);

    // outer top, three internal boundaries, outer bottom.
    assert_eq!(rules.len(), 5);
    let partial: Vec<_> = rules
        .iter()
        .filter(|rule| rule.width.0 < content_width)
        .collect();
    assert_eq!(
        partial.len(),
        1,
        "only the boundary inside the span is split"
    );
    // The boundary sits where the right-hand column starts; columns are sized
    // by content (D90), so it is no longer necessarily the middle.
    let right = of_kind(&layout, 0, BoxKind::TableCell)
        .into_iter()
        .find(|block| block_text(&layout, block) == "右下")
        .expect("bottom-right cell");
    assert_eq!(partial[0].x.0, right.x.0);
    assert_eq!(partial[0].width.0, content_width - right.x.0);
    assert_eq!(partial[0].height.0, RULE);
    for rule in rules.iter().filter(|rule| rule.width.0 == content_width) {
        assert_eq!(rule.x.0, 0);
    }
}

#[test]
fn rules_add_their_own_vertical_space() {
    let table = Table::new(
        NodeMeta::synthesized(),
        columns(1),
        vec![Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("唯一行", 1, 1)],
        )],
    );
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, RULED);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let rules = of_kind(&layout, 0, BoxKind::TableRule);
    let cells = of_kind(&layout, 0, BoxKind::TableCell);
    assert_eq!(rules.len(), 2, "top and bottom outer rules");
    assert_eq!(rules[0].y.0, 0);
    assert_eq!(rules[0].height.0, OUTER);
    assert_eq!(cells[0].y.0, OUTER);

    let bottom = rules[1];
    assert_eq!(bottom.y.0, OUTER + cells[0].height.0);
    assert_eq!(bottom.height.0, OUTER);
    assert_eq!(
        layout.pages[0].used_height.0,
        OUTER + cells[0].height.0 + OUTER
    );
}

#[test]
fn a_template_without_a_table_section_draws_no_rules() {
    let table = Table::new(
        NodeMeta::synthesized(),
        columns(1),
        vec![Row::new(
            NodeMeta::synthesized(),
            true,
            vec![cell("表头", 1, 1)],
        )],
    );
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout
        .pages
        .iter()
        .all(|page| page.blocks.iter().all(|b| b.kind != BoxKind::TableRule)));
    let header = of_kind(&layout, 0, BoxKind::TableCell);
    assert_eq!(header[0].y.0, 0, "no rule space is reserved");
}

const GRID: &str = r#"
name: grid
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 4mm
table:
  rule-width: 0
  outer-rule-width: 0
  column-rule-width: 0.4pt
  outer-column-rule-width: 0.8pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"#;

fn two_columns(rows: Vec<Row>) -> Block {
    Block::Table(Table::new(NodeMeta::synthesized(), columns(2), rows))
}

#[test]
fn a_grid_draws_vertical_rules_and_a_frame() {
    let rows = vec![
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("左上", 1, 1), cell("右上", 1, 1)],
        ),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("左下", 1, 1), cell("右下", 1, 1)],
        ),
    ];
    let doc = document(vec![two_columns(rows)]);
    let styled = styled(&doc, GRID);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let rules = of_kind(&layout, 0, BoxKind::TableRule);
    assert_eq!(rules.len(), 3, "left frame, middle rule, right frame");
    let table_width = layout.page.content_width().0;
    let column = of_kind(&layout, 0, BoxKind::TableCell)
        .into_iter()
        .find(|block| block_text(&layout, block) == "右下")
        .expect("bottom-right cell")
        .x
        .0;
    let middle = rules
        .iter()
        .find(|rule| rule.x.0 > 0 && rule.x.0 + rule.width.0 < table_width)
        .expect("middle vertical rule");
    assert_eq!(middle.width.0, RULE);
    assert_eq!(middle.x.0, column - RULE / 2);
    let height = 2 * LINE;
    assert_eq!(middle.y.0, 0);
    assert_eq!(middle.height.0, height);

    let left = rules.iter().find(|rule| rule.x.0 == 0).expect("left frame");
    assert_eq!(left.width.0, OUTER);
    assert_eq!(left.height.0, height);
    let right = rules
        .iter()
        .find(|rule| rule.x.0 + rule.width.0 == table_width)
        .expect("right frame");
    assert_eq!(right.width.0, OUTER);
    assert_eq!(right.height.0, height);
}

#[test]
fn a_vertical_rule_skips_a_colspan_cell() {
    let rows = vec![
        Row::new(NodeMeta::synthesized(), false, vec![cell("跨两列", 2, 1)]),
        Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("左下", 1, 1), cell("右下", 1, 1)],
        ),
    ];
    let doc = document(vec![two_columns(rows)]);
    let styled = styled(&doc, GRID);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let table_width = layout.page.content_width().0;
    let column = of_kind(&layout, 0, BoxKind::TableCell)
        .into_iter()
        .find(|block| block_text(&layout, block) == "右下")
        .expect("bottom-right cell")
        .x
        .0;
    let middle: Vec<_> = of_kind(&layout, 0, BoxKind::TableRule)
        .into_iter()
        .filter(|rule| rule.x.0 > 0 && rule.x.0 + rule.width.0 < table_width)
        .collect();
    assert_eq!(middle.len(), 1, "one segment below the spanning cell");
    assert_eq!(middle[0].y.0, LINE, "the span hides the first row");
    assert_eq!(middle[0].height.0, LINE);
    assert_eq!(middle[0].x.0, column - RULE / 2);
}

#[test]
fn a_dashed_rule_is_a_series_of_segments() {
    const DASHED: &str = r#"
name: dashed
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 4mm
table:
  outer-rule-width: 0.8pt
  rule-style: dashed
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"#;
    let table = Table::new(
        NodeMeta::synthesized(),
        columns(1),
        vec![Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("唯一行", 1, 1)],
        )],
    );
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, DASHED);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let rules = of_kind(&layout, 0, BoxKind::TableRule);
    assert!(
        rules.len() >= 4,
        "dashes split each outer rule: {}",
        rules.len()
    );
    assert!(rules.iter().all(|rule| rule.height.0 == OUTER));
    let dashes: Vec<_> = rules.iter().filter(|rule| rule.y.0 == 0).collect();
    assert!(dashes.len() >= 2);
    assert_eq!(dashes[0].x.0, 0);
    assert_eq!(dashes[1].x.0, (OUTER * 3).max(1) + (OUTER * 2).max(1));
}

#[test]
fn a_rule_color_rides_the_block_style() {
    const RED: &str = r##"
name: red
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 4mm
table:
  outer-rule-width: 0.8pt
  rule-color: "#aa0000"
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"##;
    let table = Table::new(
        NodeMeta::synthesized(),
        columns(1),
        vec![Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("唯一行", 1, 1)],
        )],
    );
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, RED);
    assert_eq!(
        styled.table_rules().style_id.0,
        styled.table_rules().style_id.0
    );
    let style = styled
        .style(styled.table_rules().style_id)
        .expect("rule style");
    assert_eq!(style.color, Some(0xaa0000));

    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let rules = of_kind(&layout, 0, BoxKind::TableRule);
    assert!(rules
        .iter()
        .all(|rule| rule.style == styled.table_rules().style_id));
}
