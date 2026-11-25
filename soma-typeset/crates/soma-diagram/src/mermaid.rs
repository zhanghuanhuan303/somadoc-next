//! Mermaid `flowchart` / `graph` frontend.
//!
//! This is a deliberately bounded subset of the Mermaid grammar: it accepts the
//! statements models actually emit for flowcharts, and rejects visual
//! constructs it cannot draw instead of guessing. Inert web-only directives
//! (`%%{init}%%`, `click`, `linkStyle` without geometry) are ignored.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use soma_core::Width;

use crate::ir::{Diagram, Direction, Edge, Group, LineStyle, Node, Shape, Style, Tip};
use crate::DiagramError;

const MAX_NODES: usize = 2000;
const MAX_EDGES: usize = 8000;
const MAX_GROUPS: usize = 200;

pub(crate) fn parse(source: &str) -> Result<Diagram, DiagramError> {
    let mut builder = Builder::new();
    let mut in_front_matter = false;
    let mut front_matter_done = false;

    for (index, raw) in source.lines().enumerate() {
        let line_no = index as u32 + 1;
        let trimmed = raw.trim();
        if !front_matter_done {
            if !in_front_matter && trimmed == "---" {
                in_front_matter = true;
                continue;
            }
            if in_front_matter {
                if trimmed == "---" {
                    front_matter_done = true;
                }
                continue;
            }
            front_matter_done = true;
        }
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("%%{") || trimmed.starts_with("%%") {
            continue;
        }
        let stripped = strip_comment(raw);
        for statement in split_statements(&stripped) {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            parse_statement(statement, line_no, &mut builder)?;
        }
    }

    if builder.group_stack_len() > 0 {
        return Err(DiagramError::at(0, "unclosed subgraph"));
    }
    Ok(builder.finish())
}

fn parse_statement(statement: &str, line: u32, builder: &mut Builder) -> Result<(), DiagramError> {
    if let Some(rest) = keyword(statement, "flowchart").or_else(|| keyword(statement, "graph")) {
        builder.diagram.direction = Some(parse_direction(rest).unwrap_or(Direction::TopBottom));
        return Ok(());
    }
    if let Some(rest) = keyword(statement, "subgraph") {
        return builder.open_group(rest.trim(), line);
    }
    if statement == "end" {
        return builder.close_group(line);
    }
    if let Some(rest) = keyword(statement, "classDef") {
        return builder.class_def(rest.trim(), line);
    }
    if let Some(rest) = keyword(statement, "class") {
        return builder.class_assign(rest.trim(), line);
    }
    if let Some(rest) = keyword(statement, "style") {
        return builder.node_style(rest.trim(), line);
    }
    if let Some(rest) = keyword(statement, "linkStyle") {
        return builder.link_style(rest.trim(), line);
    }
    if keyword(statement, "click").is_some()
        || keyword(statement, "accTitle").is_some()
        || keyword(statement, "accDescr").is_some()
        || keyword(statement, "direction").is_some()
    {
        return Ok(());
    }
    parse_flow_statement(statement, line, builder)
}

fn parse_direction(text: &str) -> Option<Direction> {
    let token = text.split_whitespace().next()?.to_ascii_uppercase();
    Some(match token.as_str() {
        "TD" | "TB" => Direction::TopBottom,
        "BT" => Direction::BottomTop,
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        _ => return None,
    })
}

fn keyword<'a>(statement: &'a str, word: &str) -> Option<&'a str> {
    let rest = statement.strip_prefix(word)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with(';') {
        Some(rest.trim_start_matches([' ', '\t']))
    } else {
        None
    }
}

fn strip_comment(line: &str) -> String {
    let mut quoted = false;
    let mut out = String::new();
    let mut chars = line.char_indices().peekable();
    while let Some((_index, ch)) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            out.push(ch);
            continue;
        }
        if !quoted && ch == '%' {
            if let Some((_, '%')) = chars.peek() {
                return out;
            }
        }
        out.push(ch);
    }
    out
}

fn split_statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    let mut depth = 0i32;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(String::from(&line[start..index]));
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(String::from(&line[start..]));
    out
}

/* ------------------------------------------------------------------- builder */

struct Builder {
    diagram: Diagram,
    index: BTreeMap<String, usize>,
    group_stack: Vec<usize>,
}

impl Builder {
    fn new() -> Self {
        Self {
            diagram: Diagram::default(),
            index: BTreeMap::new(),
            group_stack: Vec::new(),
        }
    }

    fn group_stack_len(&self) -> usize {
        self.group_stack.len()
    }

    fn finish(self) -> Diagram {
        self.diagram
    }

    fn ensure_node(&mut self, id: &str) -> Result<usize, DiagramError> {
        if let Some(index) = self.index.get(id) {
            return Ok(*index);
        }
        if self.diagram.nodes.len() >= MAX_NODES {
            return Err(DiagramError::at(0, "too many diagram nodes"));
        }
        let node = Node {
            id: String::from(id),
            lines: Vec::new(),
            shape: Shape::Rect,
            class: None,
            style: Style::default(),
            order: self.diagram.nodes.len(),
        };
        let index = self.diagram.nodes.len();
        self.diagram.nodes.push(node);
        self.index.insert(String::from(id), index);
        for group in &self.group_stack {
            if let Some(group) = self.diagram.groups.get_mut(*group) {
                group.members.push(index);
            }
        }
        Ok(index)
    }

    fn apply_node(&mut self, spec: NodeSpec, line: u32) -> Result<usize, DiagramError> {
        if spec.id.is_empty() {
            return Err(DiagramError::at(line, "node needs an id"));
        }
        let index = self.ensure_node(&spec.id)?;
        if let Some(shape) = spec.shape {
            self.diagram.nodes[index].shape = shape;
        }
        if let Some(lines) = spec.lines {
            self.diagram.nodes[index].lines = lines;
        }
        if let Some(class) = spec.class {
            self.diagram.nodes[index].class = Some(class);
        }
        Ok(index)
    }

    fn open_group(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        if self.diagram.groups.len() >= MAX_GROUPS {
            return Err(DiagramError::at(line, "too many subgraphs"));
        }
        let (id, title) = if let Some(open) = rest.find('[') {
            if rest.ends_with(']') {
                (
                    rest[..open].trim().to_string(),
                    parse_label(&rest[open + 1..rest.len() - 1]),
                )
            } else {
                (rest.to_string(), parse_label(rest))
            }
        } else {
            (rest.to_string(), parse_label(rest))
        };
        let id = if id.is_empty() {
            format!("subgraph{}", self.diagram.groups.len() + 1)
        } else {
            id
        };
        let parent = self.group_stack.last().copied();
        let order = self.diagram.groups.len();
        let _ = id;
        self.diagram.groups.push(Group {
            title,
            members: Vec::new(),
            parent,
            order,
        });
        self.group_stack.push(order);
        Ok(())
    }

    fn close_group(&mut self, line: u32) -> Result<(), DiagramError> {
        if self.group_stack.pop().is_none() {
            return Err(DiagramError::at(line, "`end` without a subgraph"));
        }
        Ok(())
    }

    fn class_def(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let (name, body) = split_head(rest)
            .ok_or_else(|| DiagramError::at(line, "classDef needs a name and style"))?;
        let style = parse_style(body, line)?;
        self.diagram.classes.insert(name.to_string(), style);
        Ok(())
    }

    fn class_assign(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let index = rest
            .rfind(char::is_whitespace)
            .ok_or_else(|| DiagramError::at(line, "class needs node ids and a class name"))?;
        let ids = rest[..index].trim();
        let name = rest[index..].trim();
        if ids.is_empty() || name.is_empty() {
            return Err(DiagramError::at(
                line,
                "class needs node ids and a class name",
            ));
        }
        if !self.diagram.classes.contains_key(name) {
            self.diagram
                .classes
                .insert(name.to_string(), Style::default());
        }
        for id in ids.split(|c: char| c == ',' || c.is_whitespace()) {
            let id = id.trim();
            if id.is_empty() {
                continue;
            }
            if let Some(index) = self.index.get(id).copied() {
                self.diagram.nodes[index].class = Some(name.to_string());
            }
        }
        Ok(())
    }

    fn node_style(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let (id, body) = split_head(rest)
            .ok_or_else(|| DiagramError::at(line, "style needs a node id and declarations"))?;
        let style = parse_style(body, line)?;
        if let Some(index) = self.index.get(id.trim()).copied() {
            self.diagram.nodes[index].style.merge(&style);
        }
        Ok(())
    }

    fn link_style(&mut self, rest: &str, line: u32) -> Result<(), DiagramError> {
        let (indices, body) = split_head(rest)
            .ok_or_else(|| DiagramError::at(line, "linkStyle needs indices and declarations"))?;
        let style = parse_style(body, line)?;
        for index in indices.split(',') {
            let index = index.trim();
            if index.is_empty() {
                continue;
            }
            let parsed: usize = index
                .parse()
                .map_err(|_| DiagramError::at(line, "linkStyle index must be a number"))?;
            if let Some(edge) = self.diagram.edges.get_mut(parsed) {
                edge.style.merge(&style);
            }
        }
        Ok(())
    }

    fn add_edge(
        &mut self,
        from: usize,
        to: usize,
        op: &EdgeOp,
        lines: Vec<String>,
        line: u32,
    ) -> Result<(), DiagramError> {
        if self.diagram.edges.len() >= MAX_EDGES {
            return Err(DiagramError::at(line, "too many diagram edges"));
        }
        let order = self.diagram.edges.len();
        self.diagram.edges.push(Edge {
            from,
            to,
            lines,
            line: op.line,
            start_tip: op.start_tip,
            end_tip: op.end_tip,
            span: op.span,
            visible: op.visible,
            constraint: true,
            style: Style::default(),
            order,
        });
        Ok(())
    }
}

fn split_head(text: &str) -> Option<(&str, &str)> {
    let index = text.find(char::is_whitespace)?;
    let head = text[..index].trim();
    let tail = text[index..].trim();
    (!head.is_empty() && !tail.is_empty()).then_some((head, tail))
}

/* ---------------------------------------------------------------- statements */

struct NodeSpec {
    id: String,
    shape: Option<Shape>,
    lines: Option<Vec<String>>,
    class: Option<String>,
}

fn parse_flow_statement(
    statement: &str,
    line: u32,
    builder: &mut Builder,
) -> Result<(), DiagramError> {
    let mut cursor = Cursor::new(statement, line);
    let mut previous = parse_group(&mut cursor, builder)?;
    loop {
        cursor.skip_ws();
        if cursor.rest().is_empty() {
            return Ok(());
        }
        let (op, consumed) = parse_edge_op(cursor.rest())
            .ok_or_else(|| DiagramError::at(line, "expected an edge operator"))?;
        cursor.pos += consumed;
        let mut label = op.label.as_deref().map(parse_label).unwrap_or_default();
        cursor.skip_ws();
        if cursor.eat("|") {
            let text = cursor.take_until('|')?;
            label = parse_label(text);
        }
        cursor.skip_ws();
        let next = parse_group(&mut cursor, builder)?;
        for from in &previous {
            for to in &next {
                builder.add_edge(*from, *to, &op, label.clone(), line)?;
            }
        }
        previous = next;
    }
}

fn parse_group(cursor: &mut Cursor<'_>, builder: &mut Builder) -> Result<Vec<usize>, DiagramError> {
    let mut out = Vec::new();
    loop {
        cursor.skip_ws();
        if cursor.rest().is_empty() {
            return Err(DiagramError::at(cursor.line, "expected a node"));
        }
        let spec = parse_node_spec(cursor, builder)?;
        out.push(builder.apply_node(spec, cursor.line)?);
        cursor.skip_ws();
        if cursor.eat("&") {
            continue;
        }
        return Ok(out);
    }
}

struct Cursor<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str, line: u32) -> Self {
        Self { src, pos: 0, line }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, text: &str) -> bool {
        if self.rest().starts_with(text) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    fn take_until(&mut self, closing: char) -> Result<&'a str, DiagramError> {
        let rest = self.rest();
        let mut quoted = false;
        for (offset, ch) in rest.char_indices() {
            if ch == '"' {
                quoted = !quoted;
            } else if ch == closing && !quoted {
                let text = &rest[..offset];
                self.pos += offset + ch.len_utf8();
                return Ok(text);
            }
        }
        Err(DiagramError::at(self.line, "unclosed node label"))
    }
}

fn parse_node_spec(
    cursor: &mut Cursor<'_>,
    _builder: &mut Builder,
) -> Result<NodeSpec, DiagramError> {
    cursor.skip_ws();
    let rest = cursor.rest();
    let mut end = 0usize;
    for (offset, ch) in rest.char_indices() {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            end = offset + ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return Err(DiagramError::at(
            cursor.line,
            "node id must start with a letter or digit",
        ));
    }
    let id = String::from(&rest[..end]);
    cursor.pos += end;
    let mut class = None;
    if cursor.rest().starts_with(":::") {
        cursor.pos += 3;
        let start = cursor.pos;
        while let Some(ch) = cursor.rest().chars().next() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                cursor.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if cursor.pos > start {
            class = Some(String::from(&cursor.src[start..cursor.pos]));
        }
    }
    let (shape, lines) = parse_shape(cursor)?;
    if class.is_none() && cursor.rest().starts_with(":::") {
        cursor.pos += 3;
        let start = cursor.pos;
        while let Some(ch) = cursor.rest().chars().next() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                cursor.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if cursor.pos > start {
            class = Some(String::from(&cursor.src[start..cursor.pos]));
        }
    }
    Ok(NodeSpec {
        id,
        shape,
        lines,
        class,
    })
}

fn parse_shape(
    cursor: &mut Cursor<'_>,
) -> Result<(Option<Shape>, Option<Vec<String>>), DiagramError> {
    let rest = cursor.rest();
    let (shape, open, close) = if rest.starts_with("(((") {
        (Shape::DoubleCircle, "(((", ")))")
    } else if rest.starts_with("((") {
        (Shape::Circle, "((", "))")
    } else if rest.starts_with("([") {
        (Shape::Stadium, "([", "])")
    } else if rest.starts_with("[[") {
        (Shape::Subroutine, "[[", "]]")
    } else if rest.starts_with("[(") {
        (Shape::Cylinder, "[(", ")]")
    } else if rest.starts_with("{{") {
        (Shape::Hexagon, "{{", "}}")
    } else if rest.starts_with("[/") {
        if rest.contains("\\]") {
            (Shape::Trapezoid, "[/", "\\]")
        } else {
            (Shape::Parallelogram, "[/", "/]")
        }
    } else if rest.starts_with("[\\") {
        if rest.contains("/]") {
            (Shape::TrapezoidAlt, "[\\", "/]")
        } else {
            (Shape::ParallelogramAlt, "[\\", "\\]")
        }
    } else if rest.starts_with("[") {
        (Shape::Rect, "[", "]")
    } else if rest.starts_with("(") {
        (Shape::Round, "(", ")")
    } else if rest.starts_with("{") {
        (Shape::Diamond, "{", "}")
    } else if rest.starts_with(">") {
        (Shape::Asymmetric, ">", "]")
    } else {
        return Ok((None, None));
    };
    cursor.pos += open.len();
    let text = take_shape_text(cursor, close)?;
    let text = if close.starts_with('/') || close.starts_with('\\') {
        text.trim_end_matches(['/', '\\'])
    } else {
        text
    };
    Ok((Some(shape), Some(parse_label(text))))
}

fn take_shape_text<'a>(cursor: &mut Cursor<'a>, close: &str) -> Result<&'a str, DiagramError> {
    let rest = cursor.rest();
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted && rest[offset..].starts_with(close) {
            let text = &rest[..offset];
            cursor.pos += offset + close.len();
            return Ok(text);
        }
    }
    Err(DiagramError::at(cursor.line, "unclosed node shape"))
}

/* ----------------------------------------------------------------- edge ops */

#[derive(Clone)]
struct EdgeOp {
    line: LineStyle,
    start_tip: Tip,
    end_tip: Tip,
    span: u32,
    visible: bool,
    label: Option<String>,
}

fn parse_edge_op(src: &str) -> Option<(EdgeOp, usize)> {
    if src.starts_with("~~~") {
        return Some((
            EdgeOp {
                line: LineStyle::Solid,
                start_tip: Tip::None,
                end_tip: Tip::None,
                span: 1,
                visible: false,
                label: None,
            },
            3,
        ));
    }
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut start_tip = Tip::None;
    if bytes.first() == Some(&b'<') {
        start_tip = Tip::Arrow;
        i = 1;
    }
    let mut run = 0i32;
    let mut equals = 0i32;
    let mut dots = 0i32;
    while let Some(byte) = bytes.get(i) {
        match byte {
            b'-' => run += 1,
            b'=' => equals += 1,
            b'.' => dots += 1,
            _ => break,
        }
        i += 1;
    }
    if run + equals == 0 {
        return None;
    }
    if equals > 0 && run > 0 {
        return None;
    }
    if run + equals < 2 && dots == 0 {
        return None;
    }
    let line = if equals > 0 {
        LineStyle::Thick
    } else if dots > 0 {
        LineStyle::Dashed
    } else {
        LineStyle::Solid
    };
    let span = if equals > 0 {
        (equals - 1).max(1) as u32
    } else if dots > 0 {
        (run - 1).max(1) as u32
    } else {
        (run - 2).max(1) as u32
    };
    let (end_tip, tip_len) = match bytes.get(i) {
        Some(b'>') => (Tip::Arrow, 1),
        Some(b'o') => (Tip::Circle, 1),
        Some(b'x') => (Tip::Cross, 1),
        _ => (Tip::None, 0),
    };
    if end_tip != Tip::None {
        return Some((
            EdgeOp {
                line,
                start_tip,
                end_tip,
                span,
                visible: true,
                label: None,
            },
            i + tip_len,
        ));
    }
    if src[i..].starts_with(char::is_whitespace) {
        let label_start = i;
        if let Some((offset, right)) = find_right_operator(&src[i..]) {
            let mut op = parse_compact_operator(right)?;
            let label = src[label_start..label_start + offset].trim();
            op.label = (!label.is_empty()).then(|| label.to_string());
            return Some((op, i + offset + right.len()));
        }
        let _ = label_start;
    }
    if run + equals >= 2 || dots > 0 {
        return Some((
            EdgeOp {
                line,
                start_tip,
                end_tip: Tip::None,
                span,
                visible: true,
                label: None,
            },
            i,
        ));
    }
    None
}

fn find_right_operator(src: &str) -> Option<(usize, &str)> {
    const OPS: &[&str] = &[
        "-..->", "--->", "---->", "===>", "==>", "<-->", "-.->", "---", "----", "-->", "--o",
        "--x", "==o", "==x", "-.-", "-..-",
    ];
    let mut quoted = false;
    for (offset, ch) in src.char_indices() {
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        for op in OPS {
            if src[offset..].starts_with(op) {
                return Some((offset, op));
            }
        }
    }
    None
}

fn parse_compact_operator(src: &str) -> Option<EdgeOp> {
    let (op, consumed) = parse_edge_op(src)?;
    if consumed == src.len() || src[consumed..].trim().is_empty() {
        Some(op)
    } else {
        None
    }
}

/* -------------------------------------------------------------------- labels */

fn parse_label(raw: &str) -> Vec<String> {
    let mut text = raw.trim();
    if (text.starts_with('"') && text.ends_with('"') && text.len() >= 2)
        || (text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2)
    {
        text = &text[1..text.len() - 1];
    }
    let decoded = decode_label(text);
    split_lines(&decoded)
}

pub(crate) fn decode_label(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
            continue;
        }
        if ch == '&' {
            let mut entity = String::from("&");
            let mut consumed = false;
            for _ in 0..8 {
                match chars.peek() {
                    Some(';') => {
                        chars.next();
                        entity.push(';');
                        consumed = true;
                        break;
                    }
                    Some(next) if next.is_ascii_alphanumeric() || *next == '#' => {
                        entity.push(*next);
                        chars.next();
                    }
                    _ => break,
                }
            }
            if consumed {
                out.push_str(&decode_entity(&entity));
            } else {
                out.push_str(&entity);
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "&amp;" => String::from("&"),
        "&lt;" => String::from("<"),
        "&gt;" => String::from(">"),
        "&quot;" => String::from("\""),
        "&#39;" | "&apos;" => String::from("'"),
        "&nbsp;" => String::from(" "),
        other => String::from(other),
    }
}

pub(crate) fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut rest = text;
    while let Some(index) = rest.find('<') {
        current.push_str(&rest[..index]);
        let tail = &rest[index..];
        let lower = tail.to_ascii_lowercase();
        let br = if lower.starts_with("<br/>") {
            Some(5)
        } else if lower.starts_with("<br />") {
            Some(6)
        } else if lower.starts_with("<br>") {
            Some(4)
        } else {
            None
        };
        if let Some(length) = br {
            lines.push(current.trim().to_string());
            current = String::new();
            rest = &tail[length..];
        } else {
            current.push('<');
            rest = &tail[1..];
        }
    }
    current.push_str(rest);
    for part in current.split('\n') {
        lines.push(part.trim().to_string());
    }
    while lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/* ------------------------------------------------------------------- styles */

fn parse_style(body: &str, line: u32) -> Result<Style, DiagramError> {
    let mut style = Style::default();
    for pair in split_style(body) {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair
            .split_once(':')
            .ok_or_else(|| DiagramError::at(line, "style declaration needs `key:value`"))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "fill" | "background" | "fillcolor" => {
                if value == "none" || value == "transparent" {
                    style.no_fill = true;
                } else if let Some(color) = parse_color(value) {
                    style.fill = Some(color);
                }
            }
            "stroke" | "color" | "bordercolor" => {
                if value == "none" || value == "transparent" {
                    style.no_stroke = true;
                } else if let Some(color) = parse_color(value) {
                    style.stroke = Some(color);
                }
            }
            "fontcolor" | "text" => {
                if let Some(color) = parse_color(value) {
                    style.text = Some(color);
                }
            }
            "stroke-width" | "penwidth" => {
                if let Some(width) = parse_width(value) {
                    style.width = Some(width);
                }
            }
            "stroke-dasharray" | "dash" => {
                style.dashed = Some(value != "0" && !value.is_empty());
            }
            "style" => {
                if value.contains("dashed") || value.contains("dotted") {
                    style.dashed = Some(true);
                }
            }
            "font-weight" | "font-size" | "rx" | "ry" | "filter" | "opacity" => {}
            _ => {}
        }
    }
    Ok(style)
}

fn split_style(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    for (index, ch) in body.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(&body[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(&body[start..]);
    out
}

pub(crate) fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
            }
            6 => u32::from_str_radix(hex, 16).ok(),
            _ => None,
        };
    }
    Some(match value {
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xcc0000,
        "green" => 0x00aa44,
        "blue" => 0x0044cc,
        "orange" => 0xee8800,
        "yellow" => 0xddaa00,
        "purple" => 0x8844aa,
        "gray" | "grey" => 0x888888,
        "lightgray" | "lightgrey" => 0xdddddd,
        "darkgray" | "darkgrey" => 0x444444,
        _ => return None,
    })
}

pub(crate) fn parse_width(value: &str) -> Option<Width> {
    let value = value.trim();
    let (number, factor, denominator) = if let Some(number) = value.strip_suffix("px") {
        (number, 3i64, 40_000i64)
    } else if let Some(number) = value.strip_suffix("pt") {
        (number, 1i64, 10_000i64)
    } else {
        (value, 3, 40_000)
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let whole: i64 = whole.trim().parse().ok()?;
    if whole < 0 {
        return None;
    }
    let mut milli = whole.checked_mul(1000)?;
    let mut scale = 100i64;
    for ch in fraction.chars().take(3) {
        let digit = ch.to_digit(10)? as i64;
        milli += digit * scale;
        scale /= 10;
    }
    let units = milli.checked_mul(factor)?.checked_mul(4096)? / denominator;
    Some(Width(units.min(i64::from(i32::MAX)) as i32))
}
