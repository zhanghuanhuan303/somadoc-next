//! Graphviz DOT frontend. The accepted subset is the part models emit for
//! engineering diagrams: `digraph`/`graph`, `rankdir`, node/edge attributes,
//! edge chains and `cluster_*` subgraphs. Unsupported shapes and syntax are an
//! error, never a silent approximation.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_core::Width;

use crate::ir::{Diagram, Direction, Edge, Group, LineStyle, Node, Shape, Style, Tip};
use crate::mermaid::{decode_label, parse_color, parse_width, split_lines};
use crate::DiagramError;

const MAX_NODES: usize = 2000;
const MAX_EDGES: usize = 8000;
const MAX_GROUPS: usize = 200;

pub(crate) fn parse(source: &str) -> Result<Diagram, DiagramError> {
    let mut cursor = Cursor::new(source);
    cursor.skip();
    if cursor.eat_word("strict") {
        cursor.skip();
    }
    let directed = if cursor.eat_word("digraph") {
        true
    } else if cursor.eat_word("graph") {
        cursor.eat_word("graph");
        false
    } else {
        return Err(DiagramError::at(
            0,
            "DOT source must start with digraph or graph",
        ));
    };
    cursor.skip();
    if !cursor.rest().starts_with('{') {
        let _ = cursor.parse_id()?;
        cursor.skip();
    }
    cursor.expect('{')?;
    let mut builder = Builder::new(directed);
    cursor.parse_body(&mut builder, true)?;
    if builder.stack.len() > 1 {
        return Err(DiagramError::at(0, "unclosed subgraph"));
    }
    Ok(builder.diagram)
}

struct Cursor<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn bump(&mut self, bytes: usize) {
        for ch in self.src[self.pos..self.pos + bytes].chars() {
            if ch == '\n' {
                self.line += 1;
            }
        }
        self.pos += bytes;
    }

    fn skip(&mut self) {
        loop {
            while let Some(ch) = self.rest().chars().next() {
                if ch.is_whitespace() {
                    self.bump(ch.len_utf8());
                } else {
                    break;
                }
            }
            if self.rest().starts_with("//") {
                if let Some(end) = self.rest().find('\n') {
                    self.bump(end + 1);
                } else {
                    self.pos = self.src.len();
                }
                continue;
            }
            if self.rest().starts_with("/*") {
                if let Some(end) = self.rest().find("*/") {
                    self.bump(end + 2);
                } else {
                    self.pos = self.src.len();
                }
                continue;
            }
            break;
        }
    }

    fn eat(&mut self, text: &str) -> bool {
        if self.rest().starts_with(text) {
            self.bump(text.len());
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let rest = self.rest();
        if let Some(prefix) = rest.get(..word.len()) {
            if prefix.eq_ignore_ascii_case(word) {
                let next = rest[word.len()..].chars().next();
                if next.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_' && ch != '-') {
                    self.bump(word.len());
                    return true;
                }
            }
        }
        false
    }

    fn expect(&mut self, ch: char) -> Result<(), DiagramError> {
        self.skip();
        if self.eat_char(ch) {
            Ok(())
        } else {
            Err(DiagramError::at(self.line, "expected a delimiter"))
        }
    }

    fn eat_char(&mut self, ch: char) -> bool {
        if self.rest().starts_with(ch) {
            self.bump(ch.len_utf8());
            true
        } else {
            false
        }
    }

    fn parse_id(&mut self) -> Result<String, DiagramError> {
        self.skip();
        if self.eat_char('"') {
            return self.parse_quoted();
        }
        if self.rest().starts_with('<') {
            return self.parse_html();
        }
        let start = self.pos;
        for (offset, ch) in self.rest().char_indices() {
            if ch.is_alphanumeric()
                || ch == '_'
                || ch == '.'
                || ch == '@'
                || (ch == '-'
                    && !self.rest()[offset..].starts_with("->")
                    && !self.rest()[offset..].starts_with("--"))
            {
                self.pos = start + offset + ch.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(DiagramError::at(self.line, "expected an identifier"));
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn parse_value(&mut self) -> Result<String, DiagramError> {
        self.skip();
        if self.rest().starts_with('"') {
            self.eat_char('"');
            return self.parse_quoted();
        }
        if self.rest().starts_with('<') {
            return self.parse_html();
        }
        if self.rest().starts_with('#') {
            let start = self.pos;
            while let Some(ch) = self.rest().chars().next() {
                if ch.is_whitespace() || matches!(ch, ']' | ',' | ';') {
                    break;
                }
                self.bump(ch.len_utf8());
            }
            return Ok(self.src[start..self.pos].to_string());
        }
        self.parse_id()
    }

    fn parse_quoted(&mut self) -> Result<String, DiagramError> {
        let mut out = String::new();
        let mut escaped = false;
        while let Some(ch) = self.rest().chars().next() {
            self.bump(ch.len_utf8());
            if escaped {
                out.push(match ch {
                    'n' => '\n',
                    'l' | 'r' => '\n',
                    other => other,
                });
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => return Ok(decode_label(&out)),
                other => out.push(other),
            }
        }
        Err(DiagramError::at(self.line, "unclosed string"))
    }

    fn parse_html(&mut self) -> Result<String, DiagramError> {
        self.skip();
        if !self.eat_char('<') {
            return Err(DiagramError::at(self.line, "expected an HTML label"));
        }
        let mut depth = 1usize;
        let start = self.pos;
        while let Some(ch) = self.rest().chars().next() {
            self.bump(ch.len_utf8());
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        let raw = &self.src[start..self.pos - 1];
                        return Ok(html_text(raw));
                    }
                }
                _ => {}
            }
        }
        Err(DiagramError::at(self.line, "unclosed HTML label"))
    }

    fn parse_attrs(&mut self) -> Result<Vec<(String, String)>, DiagramError> {
        let mut out = Vec::new();
        loop {
            self.skip();
            if !self.rest().starts_with('[') {
                return Ok(out);
            }
            self.eat_char('[');
            loop {
                self.skip();
                if self.eat_char(']') {
                    break;
                }
                let key = self.parse_id()?;
                self.skip();
                self.expect('=')?;
                let value = self.parse_value()?;
                out.push((key.to_ascii_lowercase(), value));
                self.skip();
                if self.eat_char(',') || self.eat_char(';') {
                    continue;
                }
                self.skip();
                if self.eat_char(']') {
                    break;
                }
                if self.rest().is_empty() {
                    return Err(DiagramError::at(self.line, "unclosed attribute list"));
                }
            }
        }
    }

    fn parse_body(&mut self, builder: &mut Builder, top: bool) -> Result<(), DiagramError> {
        loop {
            self.skip();
            if self.rest().is_empty() {
                if top {
                    return Err(DiagramError::at(self.line, "unclosed graph body"));
                }
                return Err(DiagramError::at(self.line, "unclosed subgraph"));
            }
            if self.eat_char('}') {
                if top {
                    return Ok(());
                }
                builder.pop_group();
                return Ok(());
            }
            if self.eat_char(';') {
                continue;
            }
            if self.eat_word("subgraph") {
                self.parse_subgraph(builder)?;
                continue;
            }
            if self.eat_word("node") {
                let attrs = self.parse_attrs()?;
                builder.node_defaults(&attrs);
                continue;
            }
            if self.eat_word("edge") {
                let attrs = self.parse_attrs()?;
                builder.edge_defaults(&attrs);
                continue;
            }
            if self.eat_word("graph") {
                let attrs = self.parse_attrs()?;
                builder.graph_attrs(&attrs);
                continue;
            }
            if self.eat_char('{') {
                builder.open_group(None, self.line)?;
                self.parse_body(builder, false)?;
                continue;
            }
            self.parse_statement(builder)?;
        }
    }

    fn parse_subgraph(&mut self, builder: &mut Builder) -> Result<(), DiagramError> {
        self.skip();
        let id = if self.rest().starts_with('{') {
            None
        } else {
            Some(self.parse_id()?)
        };
        self.skip();
        self.expect('{')?;
        let framed = id.as_deref().is_some_and(|id| id.starts_with("cluster"));
        builder.open_group(if framed { id } else { None }, self.line)?;
        self.parse_body(builder, false)?;
        Ok(())
    }

    fn parse_statement(&mut self, builder: &mut Builder) -> Result<(), DiagramError> {
        let first = self.parse_id()?;
        self.skip();
        // A graph-level assignment: rankdir=LR, label="...", etc.
        if self.eat_char('=') {
            let value = self.parse_value()?;
            builder.assignment(&first, &value);
            return Ok(());
        }
        if self.eat_char(':') {
            let _port = self.parse_id()?;
            self.skip();
        }
        let edge_op = if self.rest().starts_with("->") {
            Some("->")
        } else if self.rest().starts_with("--") {
            Some("--")
        } else {
            None
        };
        if let Some(op) = edge_op {
            let mut chain = vec![first];
            while self.eat(op) {
                let id = self.parse_id()?;
                if self.eat_char(':') {
                    let _port = self.parse_id()?;
                }
                self.skip();
                chain.push(id);
            }
            let attrs = self.parse_attrs()?;
            for pair in chain.windows(2) {
                builder.add_edge(&pair[0], &pair[1], &attrs, self.line)?;
            }
            return Ok(());
        }
        let attrs = self.parse_attrs()?;
        builder.add_node(&first, &attrs, self.line)?;
        Ok(())
    }
}

struct Builder {
    diagram: Diagram,
    index: BTreeMap<String, usize>,
    stack: Vec<Option<usize>>,
    node_shape: Shape,
    node_style: Style,
    edge_style: Style,
    edge_start: Tip,
    edge_end: Tip,
    edge_line: LineStyle,
}

impl Builder {
    fn new(directed: bool) -> Self {
        let diagram = Diagram {
            direction: Some(Direction::TopBottom),
            ..Diagram::default()
        };
        Self {
            diagram,
            index: BTreeMap::new(),
            stack: vec![None],
            node_shape: Shape::Ellipse,
            node_style: Style::default(),
            edge_style: Style::default(),
            edge_start: Tip::None,
            edge_end: if directed { Tip::Arrow } else { Tip::None },
            edge_line: LineStyle::Solid,
        }
    }

    fn ensure_node(&mut self, id: &str, line: u32) -> Result<usize, DiagramError> {
        if let Some(index) = self.index.get(id) {
            return Ok(*index);
        }
        if self.diagram.nodes.len() >= MAX_NODES {
            return Err(DiagramError::at(line, "too many diagram nodes"));
        }
        let index = self.diagram.nodes.len();
        self.diagram.nodes.push(Node {
            id: id.to_string(),
            lines: split_lines(id),
            shape: self.node_shape,
            class: None,
            style: self.node_style,
            order: index,
        });
        self.index.insert(id.to_string(), index);
        for group in self.stack.iter().flatten() {
            if let Some(group) = self.diagram.groups.get_mut(*group) {
                group.members.push(index);
            }
        }
        Ok(index)
    }

    fn add_node(
        &mut self,
        id: &str,
        attrs: &[(String, String)],
        line: u32,
    ) -> Result<(), DiagramError> {
        let index = self.ensure_node(id, line)?;
        let mut shape = None;
        let mut style = Style::default();
        let mut label = None;
        for (key, value) in attrs {
            match key.as_str() {
                "label" => label = Some(split_lines(value)),
                "shape" => shape = Some(parse_shape(value, line)?),
                "style" => {
                    let (parsed_shape, parsed_style) = parse_node_style(value);
                    if let Some(value_shape) = parsed_shape {
                        shape = Some(value_shape);
                    }
                    style.merge(&parsed_style);
                }
                "fillcolor" => {
                    style.fill = parse_color(value);
                }
                "color" => {
                    style.stroke = parse_color(value);
                }
                "fontcolor" => {
                    style.text = parse_color(value);
                }
                "penwidth" => {
                    style.width = parse_width(value);
                }
                _ => {}
            }
        }
        if let Some(shape) = shape {
            self.diagram.nodes[index].shape = shape;
        }
        if let Some(label) = label {
            self.diagram.nodes[index].lines = label;
        }
        self.diagram.nodes[index].style.merge(&style);
        Ok(())
    }

    fn add_edge(
        &mut self,
        from: &str,
        to: &str,
        attrs: &[(String, String)],
        line: u32,
    ) -> Result<(), DiagramError> {
        if self.diagram.edges.len() >= MAX_EDGES {
            return Err(DiagramError::at(line, "too many diagram edges"));
        }
        let from = self.ensure_node(from, line)?;
        let to = self.ensure_node(to, line)?;
        let mut style = self.edge_style;
        let mut start_tip = self.edge_start;
        let mut end_tip = self.edge_end;
        let mut line_style = self.edge_line;
        let mut lines = Vec::new();
        let mut visible = true;
        for (key, value) in attrs {
            match key.as_str() {
                "label" => lines = split_lines(value),
                "style" => {
                    let (parsed_line, parsed_style, hidden) = parse_edge_style(value);
                    if let Some(parsed) = parsed_line {
                        line_style = parsed;
                    }
                    style.merge(&parsed_style);
                    if hidden {
                        visible = false;
                    }
                }
                "color" => style.stroke = parse_color(value),
                "fontcolor" => style.text = parse_color(value),
                "penwidth" => style.width = parse_width(value),
                "arrowhead" => end_tip = parse_tip(value).unwrap_or(end_tip),
                "arrowtail" => start_tip = parse_tip(value).unwrap_or(start_tip),
                "dir" => match value.as_str() {
                    "none" => {
                        start_tip = Tip::None;
                        end_tip = Tip::None;
                    }
                    "back" => {
                        core::mem::swap(&mut start_tip, &mut end_tip);
                    }
                    "both" => {
                        if start_tip == Tip::None {
                            start_tip = Tip::Arrow;
                        }
                        if end_tip == Tip::None {
                            end_tip = Tip::Arrow;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        let constraint = !attrs
            .iter()
            .any(|(key, value)| key == "constraint" && value.eq_ignore_ascii_case("false"));
        let order = self.diagram.edges.len();
        self.diagram.edges.push(Edge {
            from,
            to,
            lines,
            line: line_style,
            start_tip,
            end_tip,
            span: 1,
            visible,
            constraint,
            style,
            order,
        });
        Ok(())
    }

    fn node_defaults(&mut self, attrs: &[(String, String)]) {
        for (key, value) in attrs {
            match key.as_str() {
                "shape" => {
                    if let Ok(shape) = parse_shape(value, 0) {
                        self.node_shape = shape;
                    }
                }
                "style" => {
                    let (shape, style) = parse_node_style(value);
                    if let Some(shape) = shape {
                        self.node_shape = shape;
                    }
                    self.node_style.merge(&style);
                }
                "fillcolor" => self.node_style.fill = parse_color(value),
                "color" => self.node_style.stroke = parse_color(value),
                "fontcolor" => self.node_style.text = parse_color(value),
                _ => {}
            }
        }
    }

    fn edge_defaults(&mut self, attrs: &[(String, String)]) {
        for (key, value) in attrs {
            match key.as_str() {
                "style" => {
                    let (line, style, _) = parse_edge_style(value);
                    if let Some(line) = line {
                        self.edge_line = line;
                    }
                    self.edge_style.merge(&style);
                }
                "color" => self.edge_style.stroke = parse_color(value),
                "penwidth" => self.edge_style.width = parse_width(value),
                "arrowhead" => self.edge_end = parse_tip(value).unwrap_or(self.edge_end),
                "arrowtail" => self.edge_start = parse_tip(value).unwrap_or(self.edge_start),
                _ => {}
            }
        }
    }

    fn graph_attrs(&mut self, attrs: &[(String, String)]) {
        for (key, value) in attrs {
            self.assignment(key, value);
        }
    }

    fn assignment(&mut self, key: &str, value: &str) {
        match key {
            "rankdir" => {
                self.diagram.direction = Some(match value.to_ascii_uppercase().as_str() {
                    "LR" => Direction::LeftRight,
                    "RL" => Direction::RightLeft,
                    "BT" => Direction::BottomTop,
                    _ => Direction::TopBottom,
                });
            }
            "label" => {
                if let Some(group) = self.stack.iter().flatten().last() {
                    if let Some(group) = self.diagram.groups.get_mut(*group) {
                        group.title = split_lines(value);
                    }
                }
            }
            _ => {}
        }
    }

    fn open_group(&mut self, id: Option<String>, line: u32) -> Result<(), DiagramError> {
        if self.diagram.groups.len() >= MAX_GROUPS {
            return Err(DiagramError::at(line, "too many subgraphs"));
        }
        let framed = id.is_some();
        let group = if framed {
            let order = self.diagram.groups.len();
            self.diagram.groups.push(Group {
                title: Vec::new(),
                members: Vec::new(),
                parent: self.stack.iter().flatten().last().copied(),
                order,
            });
            Some(order)
        } else {
            None
        };
        self.stack.push(group);
        Ok(())
    }

    fn pop_group(&mut self) {
        self.stack.pop();
    }
}

fn parse_shape(value: &str, line: u32) -> Result<Shape, DiagramError> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "box" | "rect" | "rectangle" | "square" | "note" | "component" | "folder" | "tab"
        | "box3d" => Shape::Rect,
        "ellipse" | "oval" | "egg" => Shape::Ellipse,
        "circle" | "point" | "mcircle" => Shape::Circle,
        "doublecircle" => Shape::DoubleCircle,
        "diamond" | "mdiamond" => Shape::Diamond,
        "hexagon" => Shape::Hexagon,
        "cylinder" => Shape::Cylinder,
        "parallelogram" => Shape::Parallelogram,
        "trapezium" => Shape::Trapezoid,
        "plaintext" | "none" => Shape::Rect,
        other => {
            let _ = other;
            return Err(DiagramError::at(line, "unsupported Graphviz shape"));
        }
    })
}

fn parse_node_style(value: &str) -> (Option<Shape>, Style) {
    let mut shape = None;
    let mut style = Style::default();
    let mut hidden = false;
    for token in value.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        match token {
            "" => {}
            "rounded" => shape = Some(Shape::Round),
            "filled" => {
                if style.fill.is_none() {
                    style.fill = Some(0xdddddd);
                }
            }
            "dashed" | "dotted" => style.dashed = Some(true),
            "bold" => style.width = Some(Width::from_em_frac(1, 12)),
            "invis" | "invisible" => hidden = true,
            _ => {}
        }
    }
    if hidden {
        style.no_fill = true;
        style.no_stroke = true;
        style.text = Some(0xffffff);
    }
    (shape, style)
}

fn parse_edge_style(value: &str) -> (Option<LineStyle>, Style, bool) {
    let mut line = None;
    let mut style = Style::default();
    let mut hidden = false;
    for token in value.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        match token {
            "" => {}
            "dashed" | "dotted" => line = Some(LineStyle::Dashed),
            "bold" => line = Some(LineStyle::Thick),
            "invis" | "invisible" => hidden = true,
            _ => {}
        }
    }
    if hidden {
        style.no_stroke = true;
    }
    (line, style, hidden)
}

fn parse_tip(value: &str) -> Option<Tip> {
    Some(match value.to_ascii_lowercase().as_str() {
        "none" => Tip::None,
        "normal" | "vee" | "inv" | "crow" => Tip::Arrow,
        "open" | "empty" | "halfopen" | "tee" => Tip::Open,
        "dot" | "odot" => Tip::Circle,
        "box" | "obox" | "diamond" | "odiamond" => Tip::Cross,
        _ => return None,
    })
}

fn html_text(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for next in chars.by_ref() {
                if next == '>' {
                    break;
                }
                tag.push(next);
            }
            if tag.eq_ignore_ascii_case("br")
                || tag.eq_ignore_ascii_case("br/")
                || tag.eq_ignore_ascii_case("br /")
            {
                out.push('\n');
            }
        } else {
            out.push(ch);
        }
    }
    decode_label(&out)
}
