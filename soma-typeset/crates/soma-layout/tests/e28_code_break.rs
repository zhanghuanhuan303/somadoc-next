//! ADR-0111: inline code wraps at its own spaces, never inside an identifier.

mod common;

use common::{document, engine, metrics, paragraph_with, styled, text};
use soma_doc::inline::{Code, Inline};
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::layout;

const TEMPLATE: &str = r#"
name: code-break
page:
  size: custom
  width: 64mm
  height: 200mm
  margin: 4mm
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
      first-indent: 0
"#;

/// `round(tick * tempo_num / (tempo_den * ppq))` is about 24 em in the
/// synthetic table, so it cannot fit a 15 em measure as one atom.
const EXPRESSION: &str = "round(tick * tempo_num / (tempo_den * ppq))";

fn lines(result: &soma_layout::PagedLayout) -> Vec<&soma_core::Line> {
    result
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .flat_map(|block| block.lines.iter())
        .map(|placed| &placed.line)
        .collect()
}

/// Text of each line, rebuilt from the text index (atoms keep their own text).
fn line_texts(doc: &soma_doc::Document, result: &soma_layout::PagedLayout) -> Vec<String> {
    let node = match &doc.blocks[0] {
        Block::Paragraph(paragraph) => paragraph.meta.id,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    let atoms = result
        .text
        .blocks()
        .iter()
        .find(|(id, _)| *id == node)
        .map(|(_, atoms)| atoms)
        .expect("the paragraph's text entries");
    lines(result)
        .into_iter()
        .map(|line| {
            line.boxes
                .iter()
                .map(|atom| atoms[atom.atom as usize].text.as_str())
                .collect()
        })
        .collect()
}

#[test]
fn a_long_inline_code_expression_wraps_at_its_spaces() {
    let doc = document(vec![paragraph_with(vec![
        text("调度用 "),
        Inline::Code(Code::new(NodeMeta::synthesized(), EXPRESSION)),
        text(" 计算。"),
    ])]);
    let styled = styled(&doc, TEMPLATE);
    let result = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let lines = lines(&result);

    assert!(
        lines.len() >= 3,
        "the expression must wrap, got {} lines",
        lines.len()
    );
    for line in &lines {
        assert!(
            !line.overfull,
            "an overfull line means the code could not break at a space"
        );
        assert!(
            !line.emergency,
            "an emergency line means the code could not break at a space"
        );
    }
    // The break lands after a space inside the code, and the pieces rebuild
    // the exact source text.
    let texts = line_texts(&doc, &result);
    assert!(
        texts
            .iter()
            .any(|line| line.contains("round(tick ") || line.contains("(tempo_den")),
        "{texts:?}"
    );
    assert_eq!(
        texts.concat().replace([' ', '。'], ""),
        format!("调度用{EXPRESSION}计算").replace([' ', '。'], "")
    );
}

#[test]
fn a_long_unseparated_code_token_can_wrap() {
    // 32 characters with no space, separator or case seam: only the
    // per-character intrusive break can keep the line inside the measure
    // (D114).
    const TOKEN: &str = "z7Kp2Qm9Xr4Tv8Bn1Lc6Wd3Fh5Jg0Sa";
    let doc = document(vec![paragraph_with(vec![
        text("令牌 "),
        Inline::Code(Code::new(NodeMeta::synthesized(), TOKEN)),
        text(" 结束。"),
    ])]);
    let styled = styled(&doc, TEMPLATE);
    let result = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let lines = lines(&result);

    for line in &lines {
        assert!(!line.overfull, "the token must not overfull the line");
        assert!(
            !line.emergency,
            "the token must not need the emergency stage"
        );
    }
    let texts = line_texts(&doc, &result);
    assert!(
        texts.iter().all(|line| !line.contains(TOKEN)),
        "a 16 em token cannot sit whole in a 15 em measure: {texts:?}"
    );
    assert_eq!(
        texts.concat().replace([' ', '。'], ""),
        format!("令牌{TOKEN}结束").replace([' ', '。'], "")
    );
}

#[test]
fn an_identifier_without_a_space_stays_whole() {
    // The hyphen is a priced break opportunity, not a free space: with a
    // measure wide enough for the word, no line may cut it.
    let doc = document(vec![paragraph_with(vec![
        text("基于 "),
        Inline::Code(Code::new(NodeMeta::synthesized(), "metronome-rs")),
        text(" 的代码与架构。"),
    ])]);
    let styled = styled(&doc, TEMPLATE);
    let result = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let texts = line_texts(&doc, &result);

    for line in &lines(&result) {
        assert!(!line.overfull);
    }
    assert!(
        texts.iter().any(|line| line.contains("metronome-rs")),
        "the identifier must stay on one line: {texts:?}"
    );
    assert!(
        !texts.iter().any(|line| line.ends_with("metronome-")),
        "the identifier must not be cut at its hyphen: {texts:?}"
    );
}
