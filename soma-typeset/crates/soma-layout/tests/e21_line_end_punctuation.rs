//! E21: line-end punctuation. Chinese closing punctuation draws its ink in the
//! left half of a one-em advance; the line pays only for the ink, so the line
//! end lands on the text edge instead of leaving half an em of air.
//!
//! See ADR-0036. The mechanism lives in the breaker (`trim_right`), so these
//! tests read the line geometry the renderers consume.

mod common;

use common::{document, engine, metrics, paragraph, styled, BOOK};
use soma_core::PunctClass;
use soma_layout::{layout, PlacedBlock};
use soma_rules::CLOSE_TRIM_RIGHT;

fn mixed_text() -> String {
    let unit = "中文排版，数字一二三四五六七八九十。行内标点需要压缩与悬挂。";
    unit.repeat(30)
}

fn first_block(doc: &str, template: &str) -> PlacedBlock {
    let document = document(vec![paragraph(doc)]);
    let styled = styled(&document, template);
    let result = layout(&document, &styled, &metrics(), &engine()).expect("layout");
    result.pages[0].blocks[0].clone()
}

fn text_edge(placed: &soma_layout::PlacedLine) -> i32 {
    placed.line.indent.0 + placed.line.target_width.0
}

#[test]
fn a_justified_line_drops_the_trailing_bearing() {
    let block = first_block(&mixed_text(), BOOK);
    assert!(
        block
            .lines
            .iter()
            .any(|placed| placed.line.trim_right.0 == CLOSE_TRIM_RIGHT.0),
        "a 30-sentence Chinese paragraph must end lines in punctuation"
    );
}

#[test]
fn the_ink_of_every_justified_line_lands_on_the_text_edge() {
    let block = first_block(&mixed_text(), BOOK);
    let lines = block.lines.len();

    for (index, placed) in block.lines.iter().enumerate() {
        if index + 1 == lines || placed.line.overfull {
            // The last line is left aligned; an emergency line may overrun.
            continue;
        }
        let last = placed.line.boxes.last().expect("a line has boxes");
        assert_eq!(
            last.x.0 + last.advance.0,
            text_edge(placed),
            "line {index} ends off the text edge"
        );
    }
}

#[test]
fn trimming_makes_the_line_looser_by_the_bearing() {
    // Two runs of the same paragraph with the same breakpoints: the one that
    // pays for the bearing must stretch the remaining glue further.
    let block = first_block(&mixed_text(), BOOK);
    let hung = block
        .lines
        .iter()
        .find(|placed| placed.line.trim_right.0 > 0)
        .expect("a trimmed line");

    assert!(
        hung.line.adjustment > 0,
        "an underfull line with a trimmed bearing is stretched"
    );
    assert_eq!(
        hung.line.trim_left.0, 0,
        "only the trailing mark trims here"
    );
}

#[test]
fn a_full_width_period_gives_up_half_an_em() {
    let text = mixed_text();
    let chars: Vec<char> = text.chars().collect();
    let block = first_block(&text, BOOK);

    let mut checked = 0;
    for placed in &block.lines {
        if placed.line.trim_right.0 == 0 {
            continue;
        }
        let last = placed.line.boxes.last().expect("a trimmed line has boxes");
        let end = placed.line.end as usize;
        let ch = chars[end - 1];
        assert!(
            matches!(ch, '。' | '，' | '、'),
            "only closing punctuation trims, found {ch:?}"
        );
        // Natural advance of the atom is the trimmed advance plus the trim.
        assert_eq!(
            last.advance.0 + placed.line.trim_right.0,
            4096,
            "the fixture draws every full-width mark as one em"
        );
        assert_eq!(placed.line.trim_right, CLOSE_TRIM_RIGHT);
        checked += 1;
    }
    assert!(checked > 0, "expected at least one trimmed line");
}

#[test]
fn a_line_ending_in_a_han_character_keeps_its_full_advance() {
    let block = first_block(&mixed_text(), BOOK);
    let text = mixed_text();
    let chars: Vec<char> = text.chars().collect();

    for placed in &block.lines {
        let end = placed.line.end as usize;
        let ch = chars[end - 1];
        if soma_rules::punct_class(ch) != PunctClass::None {
            continue;
        }
        assert_eq!(
            placed.line.trim_right.0, 0,
            "a Han character has no trailing bearing to trim"
        );
    }
}

#[test]
fn the_last_line_keeps_its_ink_inside_the_block() {
    let block = first_block(&mixed_text(), BOOK);
    let last = block.lines.last().expect("a paragraph has lines");
    let last_box = last.line.boxes.last().expect("a line has boxes");
    assert!(
        last_box.x.0 + last_box.advance.0 <= block.width.0,
        "a ragged last line is not stretched to the text edge"
    );
}
