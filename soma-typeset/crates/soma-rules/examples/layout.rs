//! A terminal demo of the vertical slice.
//!
//! Run with:
//!
//! ```sh
//! cargo run -p soma-rules --example layout
//! ```

use soma_core::{
    break_paragraph, break_paragraph_greedy, BoundaryKind, BreakConfig, GreedyMode, LayoutResult,
    Paragraph, StyleId, SyntheticMetrics, Width,
};
use soma_rules::{justified_style, paragraph_from_text};

const TEXT: &str =
    "中文 English，混排。中文排版的核心是把规则变成可调的代价模型，而不是在西文引擎上打补丁。";

fn line_text(paragraph: &Paragraph, line: &soma_core::Line) -> String {
    let mut out = String::new();
    for index in line.start..line.end {
        let atom = &paragraph.atoms[index as usize];
        out.push_str(&atom.text);
        if index + 1 < line.end && paragraph.boundaries[index as usize].kind == BoundaryKind::Space
        {
            out.push(' ');
        }
    }
    out
}

fn print_result(name: &str, paragraph: &Paragraph, result: &LayoutResult) {
    println!("== {name} ==");
    for (index, line) in result.lines.iter().enumerate() {
        println!(
            "line {index}: atoms {}..{}  natural={}  target={}  adj={}  cost={}{}{}",
            line.start,
            line.end,
            line.natural_width.0,
            line.target_width.0,
            line.adjustment,
            line.cost.0,
            if line.overfull {
                "  overfull"
            } else if line.adjustment < 0 {
                "  compressed"
            } else {
                ""
            },
            if line.emergency { "  emergency" } else { "" },
        );
        println!("  {}", line_text(paragraph, line));
    }
    println!(
        "total cost: {}  lines: {}\n",
        result.total_cost.0,
        result.line_count()
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let style = justified_style(Width::from_em(12));
    let paragraph = paragraph_from_text(TEXT, style, StyleId(0))?;
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let optimal = break_paragraph(&paragraph, &metrics, &config)?;
    let furthest = break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::Furthest)?;
    let min_cost = break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::MinCost)?;

    print_result("knuth-plass", &paragraph, &optimal);
    print_result("greedy-furthest", &paragraph, &furthest);
    print_result("greedy-min-cost", &paragraph, &min_cost);

    println!("== canonical result (knuth-plass) ==");
    println!("{}", optimal.canonical_json());

    Ok(())
}
