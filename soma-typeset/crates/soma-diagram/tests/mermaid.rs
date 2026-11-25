//! Flowchart layouts and the TikZ correspondence round trip (ADR-0102).

use soma_core::Width;
use soma_diagram::{layout, to_tikz, DiagramLayout, LabelLine, Options, TextEngine};
use soma_math::{MathItem, MathLayout};
use soma_tikz::{Path, Segment};

struct FakeText;

impl FakeText {
    fn measure(&self, text: &str) -> LabelLine {
        let mut width = 0;
        for ch in text.chars() {
            width += if ch.is_ascii() { 2048 } else { 4096 };
        }
        let width = Width(width);
        let ascent = Width::from_em_frac(4, 5);
        let descent = Width::from_em_frac(1, 5);
        let mut items = Vec::new();
        if !text.is_empty() {
            items.push(MathItem::text(text, Width::ZERO, Width::ZERO, Width::EM));
        }
        LabelLine {
            text: text.to_string(),
            width,
            ascent,
            descent,
            items,
            rules: Vec::new(),
        }
    }
}

impl TextEngine for FakeText {
    fn line(&mut self, text: &str) -> LabelLine {
        self.measure(text)
    }

    fn wrap(&mut self, text: &str, max_width: Width) -> Vec<LabelLine> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            if self.measure(&current).width.0 > max_width.0 && current.chars().count() > 1 {
                let mut trimmed = current.clone();
                trimmed.pop();
                lines.push(self.measure(&trimmed));
                current = String::from(ch);
            }
        }
        lines.push(self.measure(&current));
        lines
    }
}

fn compiled(source: &str) -> DiagramLayout {
    layout(
        soma_doc::DiagramDialect::Mermaid,
        source,
        &Options::default(),
        &mut FakeText,
    )
    .expect("diagram should compile")
}

#[test]
fn a_mermaid_flowchart_becomes_paths_and_labels() {
    let layout = compiled(
        "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]\n  B -->|否| D[重试]\n  D --> B\n",
    );
    assert!(layout.tikz.width.0 > 0);
    assert!(layout.tikz.height.0 > 0);
    assert!(!layout.tikz.paths.is_empty(), "nodes and edges are paths");
    assert!(layout.texts.iter().any(|text| text.text == "开始"));
    assert!(layout.texts.iter().any(|text| text.text == "是"));
}

#[test]
fn a_dot_flowchart_becomes_paths_and_labels() {
    let layout = layout(
        soma_doc::DiagramDialect::Graphviz,
        "digraph G { rankdir=LR; A [label=\"输入\"]; B [label=\"处理\", shape=diamond]; A -> B [label=\"开始\"]; }",
        &Options::default(),
        &mut FakeText,
    )
    .expect("dot should compile");
    assert!(!layout.tikz.paths.is_empty());
    assert!(layout.texts.iter().any(|text| text.text == "输入"));
    assert!(layout.texts.iter().any(|text| text.text == "开始"));
}

#[test]
fn exported_tikz_parses_back_to_the_same_paths() {
    let layout = compiled("flowchart LR\n  A[输入] --> B{处理}\n  B --> C[输出]\n");
    let tikz = to_tikz(&layout);
    let mut measure = |text: &str, scale: i64| -> Option<MathLayout> {
        let line = FakeText.measure(text);
        let size = Width::EM.scale(scale as i32, 1000);
        let mut items = line.items;
        for item in &mut items {
            item.size = size;
            item.design = size;
        }
        Some(MathLayout {
            width: line.width,
            height: line.ascent,
            depth: line.descent,
            items,
            rules: Vec::new(),
            ..MathLayout::default()
        })
    };
    let parsed = soma_tikz::layout(&tikz, 65_536, &mut measure).expect("exported TikZ parses");
    assert_eq!(parsed.paths.len(), layout.tikz.paths.len());
    assert_eq!(parsed.items.len(), layout.texts.len());
    assert!(path_points(&parsed.paths) > 0);
}

fn path_points(paths: &[Path]) -> usize {
    paths
        .iter()
        .map(|path| {
            1 + path
                .segments
                .iter()
                .filter(|segment| !matches!(segment, Segment::Close))
                .count()
        })
        .sum()
}

#[test]
fn plain_text_is_never_a_diagram() {
    assert!(soma_doc::DiagramDialect::detect(None, "plain text\nA --> B").is_none());
    assert_eq!(
        soma_doc::DiagramDialect::detect(None, "flowchart TD\nA --> B"),
        Some(soma_doc::DiagramDialect::Mermaid)
    );
    assert_eq!(
        soma_doc::DiagramDialect::detect(Some("text"), "flowchart TD\nA --> B"),
        None
    );
}

#[test]
fn directions_shapes_subgraphs_and_classes_compile() {
    for direction in ["TD", "BT", "LR", "RL"] {
        let source = format!(
            "flowchart {direction}\n  subgraph 核心\n    A([开始]) --> B[[处理]]\n  end\n  B --> C{{判断}}\n  C --> D[(库)]\n  classDef hot fill:#ffeeee,stroke:#cc0000\n  class D hot\n"
        );
        let layout = compiled(&source);
        assert!(!layout.tikz.paths.is_empty(), "{direction}");
        assert!(layout.texts.iter().any(|text| text.text == "核心"));
        assert!(!layout.texts.iter().any(|text| text.text == "missing"));
    }
}

#[test]
fn a_dot_cluster_and_self_loop_are_drawn() {
    let layout = layout(
        soma_doc::DiagramDialect::Graphviz,
        "digraph G { subgraph cluster_a { label=\"组\"; A; B; } A -> A [label=\"重试\"]; A -> B; }",
        &Options::default(),
        &mut FakeText,
    )
    .expect("dot should compile");
    assert!(layout.texts.iter().any(|text| text.text == "组"));
    assert!(layout.texts.iter().any(|text| text.text == "重试"));
    assert!(!layout.tikz.paths.is_empty());
}
