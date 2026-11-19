//! E3: hanging punctuation and pair compression at the layout seam.

mod common;

use common::{document, engine, metrics, paragraph, styled, BOOK};
use soma_layout::layout;

fn hanging_template() -> String {
    BOOK.replace(
        "first-indent: 2em",
        "first-indent: 2em\n      hanging-punctuation: true\n      hanging-cap: 0.5em",
    )
}

fn mixed_text() -> String {
    let unit = "中文排版，数字一二三四五六七八九十。行内标点需要压缩与悬挂。";
    unit.repeat(30)
}

#[test]
fn hanging_punctuation_relaxes_stretched_lines_without_moving_breaks() {
    let doc = document(vec![paragraph(&mixed_text())]);
    let base = styled(&doc, BOOK);
    let hanging = styled(&doc, &hanging_template());

    let plain = layout(&doc, &base, &metrics(), &engine()).expect("layout");
    let hung = layout(&doc, &hanging, &metrics(), &engine()).expect("layout");

    assert_eq!(
        plain.page_count(),
        hung.page_count(),
        "hanging is post-processing: it must not reflow pages"
    );

    let plain_lines: Vec<(u32, u32)> = plain.pages[0].blocks[0]
        .lines
        .iter()
        .map(|placed| (placed.line.start, placed.line.end))
        .collect();
    let hung_lines: Vec<(u32, u32)> = hung.pages[0].blocks[0]
        .lines
        .iter()
        .map(|placed| (placed.line.start, placed.line.end))
        .collect();
    assert_eq!(plain_lines, hung_lines, "breakpoints must be identical");

    assert!(
        hung.pages[0].blocks[0]
            .lines
            .iter()
            .all(|placed| placed.hang >= 0),
        "hang is never negative"
    );
    assert!(
        hung.pages[0].blocks[0]
            .lines
            .iter()
            .any(|placed| placed.hang > 0),
        "at least one stretched line ends in punctuation and must hang"
    );
    assert!(
        plain.pages[0].blocks[0]
            .lines
            .iter()
            .all(|placed| placed.hang == 0),
        "hanging off means no hang anywhere"
    );
}

#[test]
fn a_hung_line_keeps_its_boxes_inside_the_block_plus_the_cap() {
    let doc = document(vec![paragraph(&mixed_text())]);
    let styled = styled(&doc, &hanging_template());
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];
    let cap = styled
        .style(block.style)
        .map(|style| style.hanging_cap.0)
        .unwrap_or(0);

    for placed in &block.lines {
        if placed.hang == 0 {
            continue;
        }
        let last = placed.line.boxes.last().expect("a line has boxes");
        let right = last.x.0 + last.advance.0;
        assert!(
            right <= block.width.0 + placed.hang,
            "box right edge {} exceeds width {} + hang {}",
            right,
            block.width.0,
            placed.hang
        );
        assert!(placed.hang <= cap.max(1), "hang must respect the cap");
    }
}
