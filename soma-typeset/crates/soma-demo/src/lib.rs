//! A dumb HTML renderer for `LayoutResult`.
//!
//! The renderer places every atom at the integer position produced by the
//! breaker. The browser is not allowed to re-break: atoms are absolutely
//! positioned and `white-space: pre` prevents text wrapping.

use soma_core::{
    break_paragraph, break_paragraph_greedy, BreakConfig, GreedyMode, LayoutResult, Paragraph,
    ParagraphStyle, StyleId, Width,
};
use soma_layout::TableMetrics;
use soma_metrics::MetricsTable;
use soma_rules::{justified_style, paragraph_from_text};

pub const SAMPLE_TEXT: &str =
    "中文排版的核心是把规则变成可调的代价模型，而不是在西文引擎上打补丁。Mixed English 混排，数字 123。";

/// Render a paragraph twice: K-P and Greedy-F, side by side.
pub fn render_html(text: &str, width: Width) -> Result<String, String> {
    render_html_with_style(
        text,
        ParagraphStyle {
            width,
            ..justified_style(width)
        },
    )
}

/// Render with an explicit paragraph style (for first-indent demos).
pub fn render_html_with_style(text: &str, style: ParagraphStyle) -> Result<String, String> {
    let paragraph =
        paragraph_from_text(text, style, StyleId(0)).map_err(|error| error.to_string())?;
    let table = MetricsTable::embedded();
    let metrics = TableMetrics::new(&table, 0, Width::EM);
    let config = BreakConfig::default();
    let optimal =
        break_paragraph(&paragraph, &metrics, &config).map_err(|error| error.to_string())?;
    let greedy = break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::Furthest)
        .map_err(|error| error.to_string())?;

    Ok(page(&paragraph, &optimal, &greedy))
}

fn page(paragraph: &Paragraph, optimal: &LayoutResult, greedy: &LayoutResult) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>soma-typeset demo</title>\n<style>\n");
    html.push_str(
        ":root{color-scheme:light dark;}\n\
         body{margin:0;padding:2rem;font-family:\"Noto Serif CJK SC\",\"Source Han Serif SC\",serif;\
         background:#f5f3ee;color:#1d1d1f;}\n\
         h1{font-size:1.4rem;font-weight:600;margin:0 0 1rem;}\n\
         .grid{display:grid;grid-template-columns:1fr 1fr;gap:1.5rem;align-items:start;}\n\
         @media(max-width:900px){.grid{grid-template-columns:1fr;}}\n\
         .panel{background:#fff;border:1px solid #d9d5cc;border-radius:12px;padding:1.2rem;overflow:auto;}\n\
         .panel h2{font-size:1rem;margin:0 0 .35rem;}\n\
         .meta{font:12px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;color:#666;margin-bottom:1rem;}\n\
         .paragraph{position:relative;overflow:visible;}\n\
         .line{position:relative;}\n\
         .atom{position:absolute;white-space:pre;line-height:1;}\n\
         .line.emergency{background:rgba(220,80,60,.10);}\n\
         .line.overfull{outline:1px dashed rgba(220,80,60,.65);outline-offset:2px;}\n\
         details{margin-top:1.5rem;}\n\
         pre{font:11px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace;white-space:pre-wrap;\
         background:#f0eee9;border-radius:8px;padding:1rem;overflow:auto;}\n",
    );
    html.push_str("</style>\n</head>\n<body>\n");
    html.push_str("<h1>soma-typeset: LayoutResult, not browser re-breaking</h1>\n");
    html.push_str(
        "<p class=\"meta\">Every atom below is absolutely positioned from the canonical result. \
                   The browser only paints; it never chooses a break.</p>\n",
    );
    html.push_str("<div class=\"grid\">\n");
    html.push_str(&panel("optimal", "Knuth-Plass", paragraph, optimal));
    html.push_str(&panel("greedyF", "Greedy-F (furthest)", paragraph, greedy));
    html.push_str("</div>\n");

    html.push_str("<details><summary>Canonical JSON (Knuth-Plass)</summary><pre>");
    html.push_str(&escape(&optimal.canonical_json()));
    html.push_str("</pre></details>\n");
    html.push_str("</body>\n</html>\n");
    html
}

fn panel(id: &str, title: &str, paragraph: &Paragraph, result: &LayoutResult) -> String {
    let mut html = String::new();
    html.push_str(&format!(
        "<section class=\"panel\" data-algorithm=\"{id}\">\n<h2>{title}</h2>\n"
    ));
    html.push_str(&format!(
        "<div class=\"meta\">lines={} cost={} breaker={} stage={} emergency={} violations={}</div>\n",
        result.lines.len(),
        result.total_cost.0,
        result.breaker_used.as_str(),
        result.fallback_stage.as_str(),
        result.emergency_lines,
        result.forbidden_violations,
    ));
    html.push_str(&format!(
        "<div class=\"paragraph\" style=\"width:{}em\">\n",
        em(paragraph.style.width)
    ));

    for line in &result.lines {
        let mut classes = String::from("line");
        if line.emergency {
            classes.push_str(" emergency");
        }
        if line.overfull {
            classes.push_str(" overfull");
        }
        let height = if line.height.0 > 0 {
            line.height
        } else {
            paragraph.style.line_height
        };
        html.push_str(&format!(
            "<div class=\"{classes}\" style=\"height:{}em\" data-start=\"{}\" data-end=\"{}\" \
             data-target=\"{}\" data-natural=\"{}\" data-trim-left=\"{}\" data-trim-right=\"{}\" \
             data-adjustment=\"{}\" data-ratio-milli=\"{}\">\n",
            em(height),
            line.start,
            line.end,
            line.target_width.0,
            line.natural_width.0,
            line.trim_left.0,
            line.trim_right.0,
            line.adjustment,
            line.ratio_milli,
        ));

        for atom_box in &line.boxes {
            let atom = &paragraph.atoms[atom_box.atom as usize];
            let text = escape(&atom.text);
            html.push_str(&format!(
                "<span class=\"atom\" style=\"left:{}em\" data-atom=\"{}\" data-advance=\"{}\">{}</span>\n",
                em(atom_box.x),
                atom_box.atom,
                atom_box.advance.0,
                text,
            ));
        }
        html.push_str("</div>\n");
    }

    html.push_str("</div>\n</section>\n");
    html
}

/// One anonymized blind sample: the two renderings and which side is K-P.
pub struct BlindPair {
    /// HTML fragment rendered with the optimal (K-P) breaker.
    pub optimal: String,
    /// HTML fragment rendered with Greedy-F.
    pub greedy: String,
    /// `'A'` or `'B'`: the side the K-P candidate is shown on.
    pub optimal_side: char,
}

/// Break every sample twice and decide a deterministic presentation order.
///
/// The order only depends on `(seed, sample index)`, so a kit can be rebuilt
/// byte-for-byte, while a different seed swaps the sides.
pub fn blind_pairs(samples: &[&str], width: Width, seed: u64) -> Result<Vec<BlindPair>, String> {
    let style = ParagraphStyle {
        width,
        ..justified_style(width)
    };
    let table = MetricsTable::embedded();
    let metrics = TableMetrics::new(&table, 0, Width::EM);
    let config = BreakConfig::default();
    let mut pairs = Vec::with_capacity(samples.len());

    for (index, sample) in samples.iter().enumerate() {
        let paragraph =
            paragraph_from_text(sample, style, StyleId(0)).map_err(|e| e.to_string())?;
        let optimal = break_paragraph(&paragraph, &metrics, &config).map_err(|e| e.to_string())?;
        let greedy = break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::Furthest)
            .map_err(|e| e.to_string())?;
        pairs.push(BlindPair {
            optimal: fragment(&paragraph, &optimal),
            greedy: fragment(&paragraph, &greedy),
            optimal_side: if side_for(seed, index) { 'A' } else { 'B' },
        });
    }

    Ok(pairs)
}

/// Assemble the anonymized sheet. Algorithm names never appear in the output.
pub fn blind_sheet_html(pairs: &[BlindPair], width: Width) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>排版盲测</title>\n<style>\n");
    let width_em = width.0 as f64 / 4096.0;
    html.push_str(&format!(
        "body{{margin:0;padding:2rem;font-family:\"Noto Serif CJK SC\",\"Source Han Serif SC\",serif;\
         background:#f5f3ee;color:#1d1d1f;}}\n\
         h1{{font-size:1.4rem;margin:0 0 .4rem;}}\n\
         p.intro{{margin:0 0 1.6rem;color:#555;max-width:52rem;}}\n\
         .sample{{background:#fff;border:1px solid #d9d5cc;border-radius:12px;padding:1.2rem 1.4rem;\
         margin:0 0 1.2rem;overflow:auto;}}\n\
         .sample h2{{font-size:.9rem;font-weight:600;margin:0 0 .8rem;color:#666;}}\n\
         .pair{{display:grid;grid-template-columns:1fr 1fr;gap:2.2rem;align-items:start;}}\n\
         @media(max-width:1100px){{.pair{{grid-template-columns:1fr;}}}}\n\
         .side h3{{font-size:.8rem;font-weight:600;margin:0 0 .5rem;color:#888;}}\n\
         .paragraph{{position:relative;width:{}em;}}\n\
         .line{{position:relative;}}\n\
         .atom{{position:absolute;white-space:pre;line-height:1;}}\n",
        width_em
    ));
    html.push_str("</style>\n</head>\n<body>\n");
    html.push_str("<h1>排版盲测</h1>\n<p class=\"intro\">每组是同一段文字的两种断行。请只根据阅读感受选择读起来更顺的一侧，不必判断使用了什么算法。请把选择填进 ratings.csv。</p>\n");
    for (index, pair) in pairs.iter().enumerate() {
        let (left, right) = if pair.optimal_side == 'A' {
            (&pair.optimal, &pair.greedy)
        } else {
            (&pair.greedy, &pair.optimal)
        };
        html.push_str(&format!(
            "<section class=\"sample\" data-sample=\"{index}\">\n<h2>第 {} 组</h2>\n<div class=\"pair\">\n",
            index + 1
        ));
        html.push_str(&format!(
            "<div class=\"side\"><h3>A</h3>\n{left}</div>\n<div class=\"side\"><h3>B</h3>\n{right}</div>\n"
        ));
        html.push_str("</div>\n</section>\n");
    }
    html.push_str("</body>\n</html>\n");
    html
}

/// Render the sheet and the experimenter key together.
pub fn blind_sheet(
    samples: &[&str],
    width: Width,
    seed: u64,
) -> Result<(String, Vec<(usize, char)>), String> {
    let pairs = blind_pairs(samples, width, seed)?;
    let html = blind_sheet_html(&pairs, width);
    let key = pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| (index, pair.optimal_side))
        .collect();
    Ok((html, key))
}

fn fragment(paragraph: &Paragraph, result: &LayoutResult) -> String {
    let mut html = String::new();
    html.push_str(&format!(
        "<div class=\"paragraph\" style=\"width:{}em\">\n",
        em(paragraph.style.width)
    ));
    for line in &result.lines {
        let height = if line.height.0 > 0 {
            line.height
        } else {
            paragraph.style.line_height
        };
        html.push_str(&format!(
            "<div class=\"line\" style=\"height:{}em\">\n",
            em(height)
        ));
        for atom_box in &line.boxes {
            let atom = &paragraph.atoms[atom_box.atom as usize];
            html.push_str(&format!(
                "<span class=\"atom\" style=\"left:{}em\">{}</span>\n",
                em(atom_box.x),
                escape(&atom.text)
            ));
        }
        html.push_str("</div>\n");
    }
    html.push_str("</div>\n");
    html
}

/// Deterministic side choice for one sample.
fn side_for(seed: u64, index: usize) -> bool {
    let mut value = seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    (value & 1) == 1
}

fn em(width: Width) -> String {
    format!("{:.4}", width.0 as f64 / 4096.0)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_contains_both_algorithms_and_canonical_json() {
        let html = render_html(SAMPLE_TEXT, Width::from_em(20)).unwrap();
        assert!(html.contains("data-algorithm=\"optimal\""));
        assert!(html.contains("data-algorithm=\"greedyF\""));
        assert!(html.contains("soma.layout/6"));
        assert!(html.contains("position:absolute"));
        assert!(html.contains("Knuth-Plass"));
    }

    #[test]
    fn demo_escapes_source_text() {
        let html = render_html("中文 <tag> & 文本", Width::from_em(20)).unwrap();
        assert!(html.contains("&lt;"));
        assert!(html.contains("&gt;"));
        assert!(html.contains("&amp;"));
        assert!(!html.contains("<tag>"));
    }

    #[test]
    fn blind_sheet_hides_the_algorithm_and_records_the_key() {
        let samples = [
            "中文排版需要断行，标点不能在行首。",
            "Mixed English 与数字 123 也要能排。",
        ];
        let (html, key) = blind_sheet(&samples, Width::from_em(20), 7).unwrap();
        assert_eq!(key.len(), 2);
        assert!(key.iter().all(|(_, side)| *side == 'A' || *side == 'B'));
        for forbidden in ["Knuth", "Greedy", "greedy", "optimal", "K-P"] {
            assert!(
                !html.contains(forbidden),
                "{forbidden} leaked into the sheet"
            );
        }
        assert_eq!(html.matches("class=\"sample\"").count(), 2);
        assert!(html.contains(">A</h3>") && html.contains(">B</h3>"));
    }

    #[test]
    fn blind_sheet_is_deterministic_and_seed_sensitive() {
        let samples = [
            "第一段用来制造一个断行选择。",
            "第二段包含「引号」、括号与省略号……",
            "第三段 Mixed English and numbers 1234567890.",
            "第四段短一些。",
        ];
        let first = blind_sheet(&samples, Width::from_em(18), 11).unwrap();
        let again = blind_sheet(&samples, Width::from_em(18), 11).unwrap();
        assert_eq!(first.0, again.0);
        assert_eq!(first.1, again.1);

        let other = blind_sheet(&samples, Width::from_em(18), 12).unwrap();
        assert!(
            first
                .1
                .iter()
                .zip(other.1.iter())
                .any(|(left, right)| left.1 != right.1),
            "a different seed must be able to swap at least one side"
        );
    }

    #[test]
    fn blind_pairs_actually_differ() {
        let samples = [
            "中文排版的核心是把规则变成可调的代价模型，而不是在西文引擎上打补丁。",
            "标点密集：「引号」，（括号）；冒号：逗号，句号。问号？感叹号！省略号……",
            "Mixed English 与数字 123，well-known path/to/file 也要能排，不能靠溢出。",
        ];
        let pairs = blind_pairs(&samples, Width::from_em(14), 3).unwrap();
        assert_eq!(pairs.len(), 3);
        assert!(
            pairs.iter().any(|pair| pair.optimal != pair.greedy),
            "the two breakers must produce different line boxes somewhere"
        );
    }

    #[test]
    fn demo_positions_atoms_in_em_units() {
        let html = render_html("中文", Width::from_em(20)).unwrap();
        assert!(html.contains("data-atom=\"0\""));
        assert!(html.contains("left:0.0000em"));
        assert!(html.contains("data-advance=\"4096\""));
    }
}
