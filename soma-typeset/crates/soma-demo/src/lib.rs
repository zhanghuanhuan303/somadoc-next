//! A dumb HTML renderer for `LayoutResult`.
//!
//! The renderer places every atom at the integer position produced by the
//! breaker. The browser is not allowed to re-break: atoms are absolutely
//! positioned and `white-space: pre` prevents text wrapping.

use soma_core::{
    break_paragraph, break_paragraph_greedy, BreakConfig, GreedyMode, LayoutResult, Paragraph,
    ParagraphStyle, StyleId, SyntheticMetrics, Width,
};
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
    let metrics = SyntheticMetrics::default();
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
    fn demo_positions_atoms_in_em_units() {
        let html = render_html("中文", Width::from_em(20)).unwrap();
        assert!(html.contains("data-atom=\"0\""));
        assert!(html.contains("left:0.0000em"));
        assert!(html.contains("data-advance=\"4096\""));
    }
}
