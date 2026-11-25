//! The document's label engine for diagrams. Node labels use the same metrics,
//! family and CJK rules as body text; `soma-diagram` never opens a font.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use soma_core::{Metrics, PunctClass, StyleId, Width};
use soma_diagram::{LabelLine, TextEngine};
use soma_math::MathItem;
use soma_rules::{is_cjk, punct_class};
use soma_template::ComputedStyle;

/// A [`TextEngine`] backed by one computed style and the document metrics.
pub struct DiagramText<'a> {
    computed: ComputedStyle,
    metrics: &'a dyn Metrics,
    style: StyleId,
}

impl<'a> DiagramText<'a> {
    pub fn new(computed: &ComputedStyle, metrics: &'a dyn Metrics, style: StyleId) -> Self {
        Self {
            computed: computed.clone(),
            metrics,
            style,
        }
    }

    fn line_of(&self, text: &str) -> LabelLine {
        let line = self.metrics.line_metrics(self.style);
        let width = self.metrics.advance(text, self.style);
        let mut items = Vec::new();
        if !text.is_empty() {
            let mut item = MathItem::text(text, Width::ZERO, Width::ZERO, self.computed.font_size);
            item.color = self.computed.color;
            items.push(item);
        }
        LabelLine {
            text: text.to_string(),
            width,
            ascent: line.ascent,
            descent: line.descent,
            items,
            rules: Vec::new(),
        }
    }
}

impl TextEngine for DiagramText<'_> {
    fn line(&mut self, text: &str) -> LabelLine {
        self.line_of(text)
    }

    fn wrap(&mut self, text: &str, max_width: Width) -> Vec<LabelLine> {
        let cells = cells(text);
        let space = self.metrics.space_width(self.style);
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut width = Width::ZERO;
        for cell in cells {
            let cell_width = self.metrics.advance(cell.text, self.style);
            let with_space = if current.is_empty() {
                cell_width
            } else if cell.space {
                Width(space.0 + cell_width.0)
            } else {
                cell_width
            };
            if !current.is_empty() && width.0 + with_space.0 > max_width.0 && !cell.closing {
                lines.push(self.line_of(current.trim_end()));
                current.clear();
                width = Width::ZERO;
            }
            if !current.is_empty() && cell.space {
                current.push(' ');
                width = Width(width.0 + space.0);
            }
            current.push_str(cell.text);
            width = Width(width.0 + cell_width.0);
        }
        lines.push(self.line_of(current.trim_end()));
        lines
    }
}

struct Cell<'a> {
    text: &'a str,
    space: bool,
    closing: bool,
}

fn cells(text: &str) -> Vec<Cell<'_>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut token_space = false;
    let mut pending_space = false;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() {
            push_cell(text, &mut start, index, &mut token_space, &mut out);
            pending_space = true;
            continue;
        }
        if is_cjk(ch) {
            push_cell(text, &mut start, index, &mut token_space, &mut out);
            let end = index + ch.len_utf8();
            out.push(Cell {
                text: &text[index..end],
                space: pending_space,
                closing: matches!(
                    punct_class(ch),
                    PunctClass::Close | PunctClass::Middle | PunctClass::Dash
                ),
            });
            pending_space = false;
            continue;
        }
        if start.is_none() {
            start = Some(index);
            token_space = pending_space;
            pending_space = false;
        }
    }
    push_cell(text, &mut start, text.len(), &mut token_space, &mut out);
    out
}

fn push_cell<'a>(
    text: &'a str,
    start: &mut Option<usize>,
    end: usize,
    space: &mut bool,
    out: &mut Vec<Cell<'a>>,
) {
    if let Some(from) = start.take() {
        out.push(Cell {
            text: &text[from..end],
            space: *space,
            closing: false,
        });
        *space = false;
    }
}
