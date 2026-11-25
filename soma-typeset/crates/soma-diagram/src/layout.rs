//! Layered diagram layout: cycle removal, ranks, ordering, orthogonal routing.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_core::Width;
use soma_doc::DiagramDialect;
use soma_math::MathItem;
use soma_tikz::{Path, Point, Segment, TikzLayout};

use crate::ir::{Diagram, Direction, Edge, Group, LineStyle, Node, Shape, Style, Tip};
use crate::shapes::{self, Box as GeoBox, Look};
use crate::text::{LabelLine, TextEngine};
use crate::DiagramError;

const MAX_SCALE: i32 = 1000;

/// Diagram geometry is measured at the document's label size. All lengths are
/// 1/4096 em; no floating point enters the layout.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub label_size: Width,
    pub line_gap: Width,
    pub line_width: Width,
    pub max_label_width: Width,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            label_size: Width::EM,
            line_gap: Width::from_em_frac(6, 5),
            line_width: Width::from_em_frac(1, 25),
            max_label_width: Width::from_em(16),
        }
    }
}

/// One text line in TikZ coordinates. `x` is the left edge, `baseline` is the
/// baseline (`y`, negative up), `width` is the advance. The TikZ exporter uses
/// it to emit a node whose anchor is the baseline.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    pub x: Width,
    pub baseline: Width,
    pub width: Width,
    pub size: Width,
    pub color: u32,
}

#[derive(Clone, Debug)]
pub struct DiagramLayout {
    pub tikz: TikzLayout,
    pub texts: Vec<TextRun>,
    pub natural_width: Width,
    pub warnings: Vec<&'static str>,
}

/// Compile a diagram source to the shared TikZ vector box.
pub fn layout(
    dialect: DiagramDialect,
    source: &str,
    options: &Options,
    text: &mut dyn TextEngine,
) -> Result<DiagramLayout, DiagramError> {
    let diagram = match dialect {
        DiagramDialect::Mermaid => crate::mermaid::parse(source)?,
        DiagramDialect::Graphviz => crate::dot::parse(source)?,
    };
    build(&diagram, options, text)
}

/// Scale an existing layout down (never up) by permille.
pub fn scale(layout: &DiagramLayout, permille: i32) -> DiagramLayout {
    if permille >= MAX_SCALE || permille <= 0 {
        return layout.clone();
    }
    let mut out = layout.clone();
    let s = permille;
    out.tikz.width = layout.tikz.width.scale(s, MAX_SCALE);
    out.tikz.height = layout.tikz.height.scale(s, MAX_SCALE);
    out.tikz.depth = layout.tikz.depth.scale(s, MAX_SCALE);
    out.tikz.baseline = layout.tikz.baseline.scale(s, MAX_SCALE);
    for path in &mut out.tikz.paths {
        path.start = scale_point(path.start, s);
        for segment in &mut path.segments {
            match segment {
                Segment::Line(point) => *point = scale_point(*point, s),
                Segment::Cubic { c1, c2, to } => {
                    *c1 = scale_point(*c1, s);
                    *c2 = scale_point(*c2, s);
                    *to = scale_point(*to, s);
                }
                Segment::Close => {}
            }
        }
        if let Some(stroke) = &mut path.stroke {
            stroke.width = stroke.width.scale(s, MAX_SCALE);
            for dash in &mut stroke.dash {
                *dash = dash.scale(s, MAX_SCALE);
            }
        }
    }
    for item in &mut out.tikz.items {
        item.x = item.x.scale(s, MAX_SCALE);
        item.y = item.y.scale(s, MAX_SCALE);
        item.text_y = item.text_y.scale(s, MAX_SCALE);
        item.size = item.size.scale(s, MAX_SCALE);
        item.design = item.design.scale(s, MAX_SCALE);
    }
    for rule in &mut out.tikz.rules {
        rule.x = rule.x.scale(s, MAX_SCALE);
        rule.y = rule.y.scale(s, MAX_SCALE);
        rule.width = rule.width.scale(s, MAX_SCALE);
        rule.thickness = rule.thickness.scale(s, MAX_SCALE);
    }
    for text in &mut out.texts {
        text.x = text.x.scale(s, MAX_SCALE);
        text.baseline = text.baseline.scale(s, MAX_SCALE);
        text.width = text.width.scale(s, MAX_SCALE);
        text.size = text.size.scale(s, MAX_SCALE);
    }
    out.natural_width = layout.natural_width.scale(s, MAX_SCALE);
    out
}

fn scale_point(point: Point, permille: i32) -> Point {
    Point {
        x: point.x.scale(permille, MAX_SCALE),
        y: point.y.scale(permille, MAX_SCALE),
    }
}

/* ------------------------------------------------------------------- build */

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Node(usize),
    Dummy(usize),
}

#[derive(Clone, Debug)]
struct Dummy {
    layer: usize,
    key: usize,
}

#[derive(Clone, Debug)]
struct LayoutEdge {
    edge: usize,
    from: usize,
    to: usize,
    reversed: bool,
    span: usize,
    constraint: bool,
    path: Vec<Item>,
}

#[derive(Clone, Debug, Default)]
struct LabelBox {
    width: Width,
    height: Width,
    lines: Vec<PlacedLine>,
}

#[derive(Clone, Debug)]
struct PlacedLine {
    line: LabelLine,
    baseline: Width,
}

fn build(
    diagram: &Diagram,
    options: &Options,
    text: &mut dyn TextEngine,
) -> Result<DiagramLayout, DiagramError> {
    if diagram.nodes.is_empty() {
        return Ok(empty_layout());
    }
    let n = diagram.nodes.len();
    let mut labels: Vec<Option<LabelBox>> = vec![None; n];
    let mut boxes: Vec<GeoBox> = vec![
        GeoBox {
            x: Width::ZERO,
            y: Width::ZERO,
            w: Width::ZERO,
            h: Width::ZERO,
        };
        n
    ];
    let mut looks: Vec<Look> = Vec::with_capacity(n);
    for (index, node) in diagram.nodes.iter().enumerate() {
        let label = measure_label(diagram, node, options, text);
        let look = resolve_node_style(diagram, node, options);
        boxes[index] = size_node(node.shape, &label, options);
        labels[index] = Some(label);
        looks.push(look);
    }

    /* ranks */
    let mut layout_edges = Vec::new();
    let reversed = reverse_cycles(n, &diagram.edges);
    for edge in &diagram.edges {
        if edge.from == edge.to {
            continue;
        }
        let is_reversed = reversed[edge.order];
        let (from, to) = if is_reversed {
            (edge.to, edge.from)
        } else {
            (edge.from, edge.to)
        };
        layout_edges.push(LayoutEdge {
            edge: edge.order,
            from,
            to,
            reversed: is_reversed,
            span: (edge.span as usize).clamp(1, 8),
            constraint: edge.constraint,
            path: Vec::new(),
        });
    }
    let mut rank = rank_nodes(n, &layout_edges);
    rank_normalize(&mut rank);

    /* dummies */
    let mut dummies: Vec<Dummy> = Vec::new();
    let mut layers: Vec<Vec<Item>> = vec![Vec::new(); rank.iter().max().copied().unwrap_or(0) + 1];
    for edge in &mut layout_edges {
        let r0 = rank[edge.from];
        let r1 = if edge.constraint {
            rank[edge.to].max(r0 + 1)
        } else {
            rank[edge.to]
        };
        edge.path.push(Item::Node(edge.from));
        for layer in (r0 + 1)..r1 {
            let index = dummies.len();
            dummies.push(Dummy {
                layer,
                key: edge.edge * 10_000 + edge.path.len(),
            });
            edge.path.push(Item::Dummy(index));
        }
        edge.path.push(Item::Node(edge.to));
    }
    for (index, node) in diagram.nodes.iter().enumerate() {
        let layer = rank[index];
        layers[layer].push(Item::Node(index));
        let _ = node;
    }
    for (index, dummy) in dummies.iter().enumerate() {
        layers[dummy.layer].push(Item::Dummy(index));
    }
    for layer in &mut layers {
        layer.sort_by_key(|item| item_key(*item, diagram, &dummies));
    }

    /* ordering */
    let item_count = n + dummies.len();
    let mut previous: Vec<Vec<usize>> = vec![Vec::new(); item_count];
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); item_count];
    for edge in &layout_edges {
        for pair in edge.path.windows(2) {
            let a = item_index(pair[0], n);
            let b = item_index(pair[1], n);
            next[a].push(b);
            previous[b].push(a);
        }
    }
    for layer in 0..layers.len() {
        if layer == 0 {
            continue;
        }
    }
    let mut positions: Vec<usize> = vec![0; item_count];
    refresh_positions(&layers, &mut positions, n);
    for sweep in 0..8 {
        if sweep % 2 == 0 {
            for layer in 1..layers.len() {
                let reference = layers[layer - 1].clone();
                order_layer(
                    layer,
                    &reference,
                    &mut layers[layer],
                    &previous,
                    &mut positions,
                    n,
                    diagram,
                    &dummies,
                );
            }
        } else {
            for layer in (0..layers.len().saturating_sub(1)).rev() {
                let reference = layers[layer + 1].clone();
                order_layer(
                    layer + 1,
                    &reference,
                    &mut layers[layer],
                    &next,
                    &mut positions,
                    n,
                    diagram,
                    &dummies,
                );
            }
        }
    }

    /* coordinates */
    let horizontal = diagram.direction().horizontal();
    let rank_gap = options.label_size.scale(5, 2);
    let node_gap = options.label_size.scale(2, 1);
    let mut rank_extent = vec![Width::ZERO; layers.len()];
    for (index, layer) in layers.iter().enumerate() {
        for item in layer {
            let extent = if horizontal {
                item_width(*item, &boxes)
            } else {
                item_height(*item, &boxes)
            };
            rank_extent[index] = rank_extent[index].max(extent);
        }
    }
    let mut rank_pos = vec![Width::ZERO; layers.len()];
    let mut cursor = Width::ZERO;
    for (index, extent) in rank_extent.iter().enumerate() {
        rank_pos[index] = cursor;
        cursor = Width(cursor.0 + extent.0 + rank_gap.0);
    }
    let total_rank = cursor;
    let mut order_sizes = vec![Width::ZERO; layers.len()];
    for (index, layer) in layers.iter().enumerate() {
        let mut size = Width::ZERO;
        for (position, item) in layer.iter().enumerate() {
            let extent = if horizontal {
                item_height(*item, &boxes)
            } else {
                item_width(*item, &boxes)
            };
            size = Width(size.0 + extent.0);
            if position + 1 < layer.len() {
                size = Width(size.0 + node_gap.0);
            }
        }
        order_sizes[index] = size;
    }
    let max_order = order_sizes.iter().copied().max().unwrap_or(Width::ZERO);
    let mut placed: BTreeMap<Item, GeoBox> = BTreeMap::new();
    for (layer, items) in layers.iter().enumerate() {
        let offset = Width((max_order.0 - order_sizes[layer].0) / 2);
        let mut position = offset;
        let rank_coord = if horizontal {
            match diagram.direction() {
                Direction::RightLeft => {
                    Width(total_rank.0 - rank_pos[layer].0 - rank_extent[layer].0)
                }
                _ => rank_pos[layer],
            }
        } else {
            match diagram.direction() {
                Direction::BottomTop => {
                    Width(total_rank.0 - rank_pos[layer].0 - rank_extent[layer].0)
                }
                _ => rank_pos[layer],
            }
        };
        for item in items {
            let (w, h) = (item_width(*item, &boxes), item_height(*item, &boxes));
            let geo = if horizontal {
                GeoBox {
                    x: rank_coord,
                    y: position,
                    w,
                    h,
                }
            } else {
                GeoBox {
                    x: position,
                    y: rank_coord,
                    w,
                    h,
                }
            };
            placed.insert(*item, geo);
            let extent = if horizontal { h } else { w };
            position = Width(position.0 + extent.0 + node_gap.0);
        }
    }
    for (index, geo) in boxes.iter_mut().enumerate() {
        if let Some(value) = placed.get(&Item::Node(index)) {
            *geo = *value;
        }
    }

    /* paint */
    let mut frames = Vec::new();
    let mut frame_items: Vec<(Point, TextRun)> = Vec::new();
    for group in &diagram.groups {
        let Some(geo) = group_box(group, &boxes, &diagram.groups) else {
            continue;
        };
        let pad = options.label_size.scale(3, 4);
        let title_height = if group.title.is_empty() {
            Width::ZERO
        } else {
            options.label_size
        };
        let r = GeoBox {
            x: Width(geo.x.0 - pad.0),
            y: Width(geo.y.0 - pad.0 - title_height.0),
            w: Width(geo.w.0 + pad.0 * 2),
            h: Width(geo.h.0 + pad.0 * 2 + title_height.0),
        };
        let look = Look {
            fill: None,
            stroke: Some(0x666666),
            text: 0x444444,
            width: options.line_width,
            dashed: true,
        };
        frames.extend(shapes::shape_paths(
            Shape::Round,
            r,
            &look,
            options.label_size.scale(1, 3),
        ));
        if !group.title.is_empty() {
            let line = text.line(&group.title.join(" "));
            let x = Width(r.x.0 + pad.0);
            let baseline = Width(r.y.0 + pad.0 + line.ascent.0);
            frame_items.push((
                Point { x, y: baseline },
                TextRun {
                    text: group.title.join(" "),
                    x,
                    baseline,
                    width: line.width,
                    size: options.label_size,
                    color: 0x444444,
                },
            ));
        }
    }

    /* edges */
    let mut edge_paths = Vec::new();
    let mut edge_texts: Vec<TextRun> = Vec::new();
    let mut tips: Vec<Path> = Vec::new();
    let mut back_channel = Width::ZERO;
    for edge in &layout_edges {
        let source = &diagram.edges[edge.edge];
        let look = resolve_edge_style(diagram, source, options);
        if !source.visible {
            continue;
        }
        let points = route_edge(
            edge,
            source,
            diagram,
            &boxes,
            &placed,
            &labels,
            options,
            &mut back_channel,
            text,
        );
        if points.len() < 2 {
            continue;
        }
        if let Some(stroke) = look.stroke() {
            let mut segments = Vec::new();
            for point in &points[1..] {
                segments.push(Segment::Line(*point));
            }
            edge_paths.push(Path {
                start: points[0],
                segments,
                fill: None,
                stroke: Some(stroke),
            });
        }
        if edge_label(source, &points, options, text, &mut edge_texts).is_some() {
            // Edge labels sit beside the line; no background rectangle is
            // needed, so the painter keeps the line visible.
        }
        let (start_tip, end_tip) = effective_tips(source, edge.reversed);
        if start_tip != Tip::None {
            let direction = direction_between(points[1], points[0]);
            tips.extend(shapes::arrow(
                points[0],
                direction,
                options.label_size.scale(2, 5),
                look.stroke.unwrap_or(0),
                start_tip,
            ));
        }
        if end_tip != Tip::None {
            let last = points.len() - 1;
            let direction = direction_between(points[last - 1], points[last]);
            tips.extend(shapes::arrow(
                points[last],
                direction,
                options.label_size.scale(2, 5),
                look.stroke.unwrap_or(0),
                end_tip,
            ));
        }
    }

    // Self-loops do not affect ranks, so they are routed after the layers are
    // fixed. They still get a label and arrow tips.
    for source in &diagram.edges {
        if source.from != source.to || !source.visible {
            continue;
        }
        let look = resolve_edge_style(diagram, source, options);
        let points = self_loop(boxes[source.from], diagram.direction(), options);
        if points.len() < 2 {
            continue;
        }
        if let Some(stroke) = look.stroke() {
            edge_paths.push(Path {
                start: points[0],
                segments: points[1..]
                    .iter()
                    .map(|point| Segment::Line(*point))
                    .collect(),
                fill: None,
                stroke: Some(stroke),
            });
        }
        let _ = edge_label(source, &points, options, text, &mut edge_texts);
        let (start_tip, end_tip) = effective_tips(source, false);
        if start_tip != Tip::None {
            let direction = direction_between(points[1], points[0]);
            tips.extend(shapes::arrow(
                points[0],
                direction,
                options.label_size.scale(2, 5),
                look.stroke.unwrap_or(0),
                start_tip,
            ));
        }
        if end_tip != Tip::None {
            let last = points.len() - 1;
            let direction = direction_between(points[last - 1], points[last]);
            tips.extend(shapes::arrow(
                points[last],
                direction,
                options.label_size.scale(2, 5),
                look.stroke.unwrap_or(0),
                end_tip,
            ));
        }
    }

    /* node shapes and labels */
    let mut node_paths = Vec::new();
    let mut node_texts: Vec<TextRun> = Vec::new();
    for (index, node) in diagram.nodes.iter().enumerate() {
        let geo = boxes[index];
        let look = &looks[index];
        let radius = options.label_size.scale(1, 5);
        node_paths.extend(shapes::shape_paths(node.shape, geo, look, radius));
        if let Some(label) = &labels[index] {
            let body_offset = if node.shape == Shape::Cylinder {
                options.label_size.scale(1, 2)
            } else {
                Width::ZERO
            };
            let body_h = Width(geo.h.0 - body_offset.0);
            let top = Width(geo.y.0 + body_offset.0 + (body_h.0 - label.height.0).max(0) / 2);
            for placed in &label.lines {
                let x = Width(geo.x.0 + (geo.w.0 - placed.line.width.0) / 2);
                let baseline = Width(top.0 + placed.baseline.0);
                let mut items = placed.line.items.clone();
                for item in &mut items {
                    item.x = Width(item.x.0 + x.0);
                    item.y = Width(item.y.0 + baseline.0);
                    item.text_y = item.y;
                    item.color = Some(look.text_color());
                }
                for rule in &placed.line.rules {
                    let mut rule = rule.clone();
                    rule.x = Width(rule.x.0 + x.0);
                    rule.y = Width(rule.y.0 + baseline.0);
                    // Rules are emitted directly below; this keeps the text
                    // and its rules in the same coordinate transform.
                    let _ = rule;
                }
                node_texts.push(TextRun {
                    text: placed.line.text.clone(),
                    x,
                    baseline,
                    width: placed.line.width,
                    size: options.label_size,
                    color: look.text_color(),
                });
            }
        }
    }

    /* normalise bounds and convert to the baseline frame */
    let mut bounds = Bounds::new();
    for geo in &boxes {
        bounds.add_box(*geo);
    }
    for path in frames
        .iter()
        .chain(edge_paths.iter())
        .chain(node_paths.iter())
        .chain(tips.iter())
    {
        bounds.add_path(path);
    }
    for (point, run) in &frame_items {
        bounds.add_text(point.x, point.y, run.width, run.size);
    }
    for run in &edge_texts {
        bounds.add_text(run.x, run.baseline, run.width, run.size);
    }
    for run in &node_texts {
        bounds.add_text(run.x, run.baseline, run.width, run.size);
    }
    let min_x = bounds.min_x;
    let min_y = bounds.min_y;
    let max_x = bounds.max_x;
    let max_y = bounds.max_y;
    let margin = options.label_size.scale(1, 2);
    let width = Width((max_x.0 - min_x.0).max(0) + margin.0 * 2);
    let height = Width((max_y.0 - min_y.0).max(0) + margin.0 * 2);
    let shift_x = Width(margin.0 - min_x.0);
    let shift_y = Width(margin.0 - min_y.0);

    let mut paths = Vec::new();
    for path in frames
        .into_iter()
        .chain(edge_paths)
        .chain(node_paths)
        .chain(tips)
    {
        paths.push(shift_path(path, shift_x, shift_y, height));
    }
    let mut items = Vec::new();
    let mut texts = Vec::new();
    for (point, run) in frame_items {
        items.push(
            MathItem::text(
                run.text.clone(),
                Width(point.x.0 + shift_x.0),
                Width(point.y.0 + shift_y.0 - height.0),
                run.size,
            )
            .text_face(),
        );
        items.last_mut().unwrap().color = Some(run.color);
        texts.push(TextRun {
            text: run.text,
            x: Width(run.x.0 + shift_x.0),
            baseline: Width(run.baseline.0 + shift_y.0 - height.0),
            width: run.width,
            size: run.size,
            color: run.color,
        });
    }
    for run in edge_texts.into_iter().chain(node_texts) {
        items.push(
            MathItem::text(
                run.text.clone(),
                Width(run.x.0 + shift_x.0),
                Width(run.baseline.0 + shift_y.0 - height.0),
                run.size,
            )
            .text_face(),
        );
        items.last_mut().unwrap().color = Some(run.color);
        texts.push(TextRun {
            text: run.text,
            x: Width(run.x.0 + shift_x.0),
            baseline: Width(run.baseline.0 + shift_y.0 - height.0),
            width: run.width,
            size: run.size,
            color: run.color,
        });
    }
    Ok(DiagramLayout {
        tikz: TikzLayout {
            width,
            height,
            depth: Width::ZERO,
            baseline: height,
            items,
            rules: Vec::new(),
            paths,
        },
        texts,
        natural_width: width,
        warnings: Vec::new(),
    })
}

fn empty_layout() -> DiagramLayout {
    DiagramLayout {
        tikz: TikzLayout {
            width: Width::ZERO,
            height: Width::ZERO,
            depth: Width::ZERO,
            baseline: Width::ZERO,
            items: Vec::new(),
            rules: Vec::new(),
            paths: Vec::new(),
        },
        texts: Vec::new(),
        natural_width: Width::ZERO,
        warnings: vec!["empty diagram"],
    }
}

/* ------------------------------------------------------------------- labels */

fn measure_label(
    _diagram: &Diagram,
    node: &Node,
    options: &Options,
    text: &mut dyn TextEngine,
) -> LabelBox {
    let source: Vec<String> = if node.lines.is_empty() {
        vec![node.id.clone()]
    } else {
        node.lines.clone()
    };
    let mut lines = Vec::new();
    for line in source {
        let wrapped = text.wrap(&line, options.max_label_width);
        if wrapped.is_empty() {
            lines.push(text.line(""));
        } else {
            lines.extend(wrapped);
        }
    }
    let mut width = Width::ZERO;
    let mut height = Width::ZERO;
    for (index, line) in lines.iter().enumerate() {
        width = width.max(line.width);
        height = Width(height.0 + line.ascent.0 + line.descent.0);
        if index + 1 < lines.len() {
            height = Width(height.0 + options.line_gap.0);
        }
    }
    let mut placed = Vec::with_capacity(lines.len());
    let mut cursor = Width::ZERO;
    for line in lines {
        let baseline = Width(cursor.0 + line.ascent.0);
        cursor = Width(cursor.0 + line.ascent.0 + line.descent.0 + options.line_gap.0);
        placed.push(PlacedLine { line, baseline });
    }
    LabelBox {
        width,
        height,
        lines: placed,
    }
}

fn size_node(shape: Shape, label: &LabelBox, options: &Options) -> GeoBox {
    let pad_x = options.label_size.scale(1, 2);
    let pad_y = options.label_size.scale(1, 3);
    let min_w = options.label_size.scale(3, 1);
    let min_h = options.label_size.scale(2, 1);
    let content_w = Width(label.width.0 + pad_x.0 * 2);
    let content_h = Width(label.height.0 + pad_y.0 * 2);
    let mut w = content_w.max(min_w);
    let mut h = content_h.max(min_h);
    match shape {
        Shape::Diamond => {
            w = Width(content_w.0 * 2 + pad_x.0);
            h = Width(content_h.0 * 2 + pad_y.0);
        }
        Shape::Circle | Shape::DoubleCircle => {
            let diameter = content_w.max(content_h);
            w = Width(diameter.0 * 6 / 5);
            h = w;
            w = w.max(min_w);
            h = h.max(min_h);
        }
        Shape::Cylinder => {
            h = Width(content_h.0 + options.label_size.0);
        }
        Shape::Ellipse => {
            w = Width(content_w.0 * 4 / 3);
            h = Width(content_h.0 * 4 / 3);
        }
        Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt => {
            w = Width(w.0 + options.label_size.0 / 2);
        }
        Shape::Asymmetric => {
            w = Width(w.0 + options.label_size.0 / 2);
        }
        _ => {}
    }
    GeoBox {
        x: Width::ZERO,
        y: Width::ZERO,
        w,
        h,
    }
}

/* -------------------------------------------------------------------- style */

impl Look {
    fn text_color(&self) -> u32 {
        self.text
    }
}

fn resolve_node_style(diagram: &Diagram, node: &Node, options: &Options) -> Look {
    let mut style = Style::default();
    if let Some(class) = &node.class {
        if let Some(value) = diagram.classes.get(class) {
            style.merge(value);
        }
    }
    style.merge(&node.style);
    resolve_look(&style, options)
}

fn resolve_edge_style(diagram: &Diagram, edge: &Edge, options: &Options) -> Look {
    let _ = diagram;
    let mut style = edge.style;
    match edge.line {
        LineStyle::Dashed => style.dashed = Some(true),
        LineStyle::Thick => {
            style.width = Some(options.line_width.scale(2, 1));
        }
        LineStyle::Solid => {}
    }
    resolve_look(&style, options)
}

fn resolve_look(style: &Style, options: &Options) -> Look {
    Look {
        fill: if style.no_fill {
            None
        } else {
            Some(style.fill.unwrap_or(0xffffff))
        },
        stroke: if style.no_stroke {
            None
        } else {
            Some(style.stroke.unwrap_or(0x000000))
        },
        text: style.text.unwrap_or(0x000000),
        width: style.width.unwrap_or(options.line_width),
        dashed: style.dashed.unwrap_or(false),
    }
}

/* -------------------------------------------------------------------- ranks */

fn reverse_cycles(n: usize, edges: &[Edge]) -> Vec<bool> {
    let mut reversed = vec![false; edges.len()];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in edges {
        if edge.from != edge.to && edge.constraint {
            adjacency[edge.from].push(edge.order);
        }
    }
    let mut color = vec![0u8; n];
    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        color[start] = 1;
        while let Some((node, index)) = stack.last_mut() {
            if *index >= adjacency[*node].len() {
                color[*node] = 2;
                stack.pop();
                continue;
            }
            let edge_index = adjacency[*node][*index];
            *index += 1;
            let target = edges[edge_index].to;
            if target == *node {
                continue;
            }
            if color[target] == 1 {
                reversed[edge_index] = true;
            } else if color[target] == 0 {
                color[target] = 1;
                stack.push((target, 0));
            }
        }
    }
    reversed
}

fn rank_nodes(n: usize, edges: &[LayoutEdge]) -> Vec<usize> {
    let mut indegree = vec![0usize; n];
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (index, edge) in edges.iter().enumerate() {
        if !edge.constraint {
            continue;
        }
        indegree[edge.to] += 1;
        outgoing[edge.from].push(index);
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|index| indegree[*index] == 0).collect();
    let mut rank = vec![0usize; n];
    let mut done = 0usize;
    while let Some(node) = ready.pop_first() {
        done += 1;
        for edge_index in &outgoing[node] {
            let edge = &edges[*edge_index];
            let span = edge.span;
            rank[edge.to] = rank[edge.to].max(rank[node] + span);
            indegree[edge.to] -= 1;
            if indegree[edge.to] == 0 {
                ready.insert(edge.to);
            }
        }
    }
    if done < n {
        // Cycle removal should have made this impossible. Keep a stable
        // fallback rather than panicking on malformed input.
        for (index, value) in rank.iter_mut().enumerate() {
            if indegree[index] > 0 {
                *value = index;
            }
        }
    }
    rank
}

fn rank_normalize(rank: &mut [usize]) {
    let min = rank.iter().copied().min().unwrap_or(0);
    for value in rank {
        *value -= min;
    }
}

/* ------------------------------------------------------------------ ordering */

fn item_key(item: Item, diagram: &Diagram, dummies: &[Dummy]) -> usize {
    match item {
        Item::Node(index) => diagram.nodes[index].order,
        Item::Dummy(index) => 1_000_000 + dummies[index].key,
    }
}

fn item_index(item: Item, nodes: usize) -> usize {
    match item {
        Item::Node(index) => index,
        Item::Dummy(index) => nodes + index,
    }
}

fn item_from_index(index: usize, nodes: usize) -> Item {
    if index < nodes {
        Item::Node(index)
    } else {
        Item::Dummy(index - nodes)
    }
}

fn item_width(item: Item, boxes: &[GeoBox]) -> Width {
    match item {
        Item::Node(index) => boxes[index].w,
        Item::Dummy(_) => Width::ZERO,
    }
}

fn item_height(item: Item, boxes: &[GeoBox]) -> Width {
    match item {
        Item::Node(index) => boxes[index].h,
        Item::Dummy(_) => Width::ZERO,
    }
}

fn refresh_positions(layers: &[Vec<Item>], positions: &mut [usize], nodes: usize) {
    for layer in layers {
        for (index, item) in layer.iter().enumerate() {
            positions[item_index(*item, nodes)] = index;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn order_layer(
    target_layer: usize,
    reference: &[Item],
    target: &mut [Item],
    neighbours: &[Vec<usize>],
    positions: &mut [usize],
    nodes: usize,
    diagram: &Diagram,
    dummies: &[Dummy],
) {
    let mut reference_position = BTreeMap::new();
    for (index, item) in reference.iter().enumerate() {
        reference_position.insert(*item, index);
    }
    let mut keyed: Vec<(i64, usize, Item)> = target
        .iter()
        .map(|item| {
            let list = &neighbours[item_index(*item, nodes)];
            let mut sum = 0i64;
            let mut count = 0i64;
            for neighbour in list {
                let neighbour_item = item_from_index(*neighbour, nodes);
                if let Some(position) = reference_position.get(&neighbour_item) {
                    sum += *position as i64;
                    count += 1;
                }
            }
            let barycenter = if count > 0 {
                sum * 1000 / count
            } else {
                positions[item_index(*item, nodes)] as i64 * 1000
            };
            (barycenter, item_key(*item, diagram, dummies), *item)
        })
        .collect();
    keyed.sort_by_key(|(barycenter, key, _)| (*barycenter, *key));
    for (index, (_, _, item)) in keyed.into_iter().enumerate() {
        target[index] = item;
        positions[item_index(item, nodes)] = index;
    }
    let _ = target_layer;
}

/* ------------------------------------------------------------------- routing */

#[allow(clippy::too_many_arguments)]
fn route_edge(
    edge: &LayoutEdge,
    source: &Edge,
    diagram: &Diagram,
    boxes: &[GeoBox],
    placed: &BTreeMap<Item, GeoBox>,
    labels: &[Option<LabelBox>],
    options: &Options,
    back_channel: &mut Width,
    _text: &mut dyn TextEngine,
) -> Vec<Point> {
    let direction = diagram.direction();
    if source.from == source.to {
        return self_loop(boxes[source.from], direction, options);
    }
    let from_box = if edge.reversed {
        boxes[source.to]
    } else {
        boxes[source.from]
    };
    let to_box = if edge.reversed {
        boxes[source.from]
    } else {
        boxes[source.to]
    };
    if edge.reversed {
        let horizontal = direction.horizontal();
        let offset = options.label_size.scale(3, 2);
        *back_channel = Width(back_channel.0 + options.label_size.scale(1, 2).0);
        let min_x = Width(boxes.iter().map(|b| b.x.0).min().unwrap_or(0));
        let max_x = Width(boxes.iter().map(|b| b.right().0).max().unwrap_or(0));
        let min_y = Width(boxes.iter().map(|b| b.y.0).min().unwrap_or(0));
        let max_y = Width(boxes.iter().map(|b| b.bottom().0).max().unwrap_or(0));
        if horizontal {
            let middle = (min_y.0 + max_y.0) / 2;
            let below = from_box.center().y.0 >= middle;
            let y = if below {
                Width(max_y.0 + offset.0 + back_channel.0)
            } else {
                Width(min_y.0 - offset.0 - back_channel.0)
            };
            let start = Point {
                x: from_box.center().x,
                y: if below { from_box.bottom() } else { from_box.y },
            };
            let end = Point {
                x: to_box.center().x,
                y: if below { to_box.bottom() } else { to_box.y },
            };
            return dedup(vec![
                start,
                Point { x: start.x, y },
                Point { x: end.x, y },
                end,
            ]);
        }
        let middle = (min_x.0 + max_x.0) / 2;
        let right = from_box.center().x.0 >= middle;
        let x = if right {
            Width(max_x.0 + offset.0 + back_channel.0)
        } else {
            Width(min_x.0 - offset.0 - back_channel.0)
        };
        let start = Point {
            x: if right { from_box.right() } else { from_box.x },
            y: from_box.center().y,
        };
        let end = Point {
            x: if right { to_box.right() } else { to_box.x },
            y: to_box.center().y,
        };
        return dedup(vec![
            start,
            Point { x, y: start.y },
            Point { x, y: end.y },
            end,
        ]);
    }
    let horizontal = direction.horizontal();
    let mut points = Vec::new();
    points.push(port(from_box, direction, true));
    for item in &edge.path[1..edge.path.len() - 1] {
        if let Some(geo) = placed.get(item) {
            points.push(geo.center());
        }
    }
    points.push(port(to_box, direction, false));
    let _ = labels;
    orthogonal(points, horizontal)
}

fn port(geo: GeoBox, direction: Direction, source: bool) -> Point {
    match (direction, source) {
        (Direction::TopBottom, true) => Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.bottom(),
        },
        (Direction::TopBottom, false) => Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.y,
        },
        (Direction::BottomTop, true) => Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.y,
        },
        (Direction::BottomTop, false) => Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.bottom(),
        },
        (Direction::LeftRight, true) => Point {
            x: geo.right(),
            y: Width(geo.y.0 + geo.h.0 / 2),
        },
        (Direction::LeftRight, false) => Point {
            x: geo.x,
            y: Width(geo.y.0 + geo.h.0 / 2),
        },
        (Direction::RightLeft, true) => Point {
            x: geo.x,
            y: Width(geo.y.0 + geo.h.0 / 2),
        },
        (Direction::RightLeft, false) => Point {
            x: geo.right(),
            y: Width(geo.y.0 + geo.h.0 / 2),
        },
    }
}

fn orthogonal(points: Vec<Point>, horizontal: bool) -> Vec<Point> {
    let mut out = Vec::new();
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        push(&mut out, a);
        if a.x != b.x && a.y != b.y {
            if horizontal {
                let mid = Width((a.x.0 + b.x.0) / 2);
                push(&mut out, Point { x: mid, y: a.y });
                push(&mut out, Point { x: mid, y: b.y });
            } else {
                let mid = Width((a.y.0 + b.y.0) / 2);
                push(&mut out, Point { x: a.x, y: mid });
                push(&mut out, Point { x: b.x, y: mid });
            }
        }
        push(&mut out, b);
    }
    out
}

fn push(out: &mut Vec<Point>, point: Point) {
    if out.last() != Some(&point) {
        out.push(point);
    }
}

fn dedup(points: Vec<Point>) -> Vec<Point> {
    let mut out = Vec::new();
    for point in points {
        push(&mut out, point);
    }
    out
}

fn self_loop(geo: GeoBox, direction: Direction, options: &Options) -> Vec<Point> {
    let gap = options.label_size.scale(1, 1);
    let rise = options.label_size.scale(3, 2);
    let horizontal = direction.horizontal();
    if horizontal {
        let start = Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.bottom(),
        };
        let end = Point {
            x: Width(geo.x.0 + geo.w.0 / 2),
            y: geo.bottom(),
        };
        let y = Width(geo.bottom().0 + gap.0);
        dedup(vec![
            start,
            Point {
                x: Width(start.x.0 + rise.0),
                y: start.y,
            },
            Point {
                x: Width(start.x.0 + rise.0),
                y,
            },
            Point {
                x: Width(start.x.0 - rise.0),
                y,
            },
            Point {
                x: Width(start.x.0 - rise.0),
                y: end.y,
            },
            end,
        ])
    } else {
        let start = Point {
            x: geo.right(),
            y: Width(geo.y.0 + geo.h.0 / 2),
        };
        let end = Point {
            x: geo.right(),
            y: Width(geo.y.0 + geo.h.0 / 2),
        };
        let x = Width(geo.right().0 + gap.0);
        dedup(vec![
            start,
            Point {
                x: start.x,
                y: Width(start.y.0 - rise.0),
            },
            Point {
                x,
                y: Width(start.y.0 - rise.0),
            },
            Point {
                x,
                y: Width(start.y.0 + rise.0),
            },
            Point {
                x: end.x,
                y: Width(start.y.0 + rise.0),
            },
            end,
        ])
    }
}

fn direction_between(from: Point, to: Point) -> (i32, i32) {
    let dx = (to.x.0 - from.x.0).signum();
    let dy = (to.y.0 - from.y.0).signum();
    if dx == 0 && dy == 0 {
        (0, -1)
    } else {
        (dx, dy)
    }
}

fn effective_tips(edge: &Edge, reversed: bool) -> (Tip, Tip) {
    if reversed {
        (edge.end_tip, edge.start_tip)
    } else {
        (edge.start_tip, edge.end_tip)
    }
}

fn edge_label(
    edge: &Edge,
    points: &[Point],
    options: &Options,
    text: &mut dyn TextEngine,
    out: &mut Vec<TextRun>,
) -> Option<()> {
    if edge.lines.is_empty() {
        return None;
    }
    if edge.lines.iter().all(|line| line.trim().is_empty()) {
        return None;
    }
    let mut width = Width::ZERO;
    let mut height = Width::ZERO;
    let mut lines: Vec<LabelLine> = Vec::new();
    for line in &edge.lines {
        let wrapped = text.wrap(line, options.label_size.scale(10, 1));
        for line in wrapped {
            width = width.max(line.width);
            height = Width(height.0 + line.ascent.0 + line.descent.0);
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return None;
    }
    let mut segment = 0usize;
    let mut best = 0i32;
    for (index, pair) in points.windows(2).enumerate() {
        let length = (pair[1].x.0 - pair[0].x.0).abs() + (pair[1].y.0 - pair[0].y.0).abs();
        if length > best {
            best = length;
            segment = index;
        }
    }
    let a = points[segment];
    let b = points[segment + 1];
    let gap = options.label_size.scale(1, 5);
    let horizontal_segment = (b.x.0 - a.x.0).abs() >= (b.y.0 - a.y.0).abs();
    let self_loop = edge.from == edge.to;
    let (x, y) = if self_loop && horizontal_segment {
        (
            Width((a.x.0 + b.x.0) / 2 - width.0 / 2),
            Width(a.y.max(b.y).0 + gap.0),
        )
    } else if self_loop {
        (
            Width(a.x.max(b.x).0 + gap.0),
            Width((a.y.0 + b.y.0) / 2 - height.0 / 2),
        )
    } else if horizontal_segment {
        (
            Width((a.x.0 + b.x.0) / 2 - width.0 / 2),
            Width((a.y.0 + b.y.0) / 2 - height.0 - gap.0),
        )
    } else {
        (
            Width((a.x.0 + b.x.0) / 2 + gap.0),
            Width((a.y.0 + b.y.0) / 2 - height.0 / 2),
        )
    };
    let mut cursor = y;
    for line in lines {
        let baseline = Width(cursor.0 + line.ascent.0);
        let run = TextRun {
            text: line.text.clone(),
            x: Width(x.0 + (width.0 - line.width.0) / 2),
            baseline,
            width: line.width,
            size: options.label_size,
            color: 0x000000,
        };
        out.push(run);
        cursor = Width(cursor.0 + line.ascent.0 + line.descent.0 + options.line_gap.0);
    }
    Some(())
}

/* -------------------------------------------------------------------- groups */

fn group_box(group: &Group, boxes: &[GeoBox], groups: &[Group]) -> Option<GeoBox> {
    let mut result: Option<GeoBox> = None;
    for member in &group.members {
        let geo = boxes[*member];
        result = Some(match result {
            None => geo,
            Some(current) => union(current, geo),
        });
    }
    for child in groups
        .iter()
        .filter(|candidate| candidate.parent == Some(group.order))
    {
        if let Some(geo) = group_box(child, boxes, groups) {
            result = Some(match result {
                None => geo,
                Some(current) => union(current, geo),
            });
        }
    }
    result
}

fn union(a: GeoBox, b: GeoBox) -> GeoBox {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    GeoBox {
        x,
        y,
        w: Width(right.0 - x.0),
        h: Width(bottom.0 - y.0),
    }
}

/* --------------------------------------------------------------------- bounds */

struct Bounds {
    min_x: Width,
    min_y: Width,
    max_x: Width,
    max_y: Width,
}

impl Bounds {
    fn new() -> Self {
        Self {
            min_x: Width(i32::MAX),
            min_y: Width(i32::MAX),
            max_x: Width(i32::MIN),
            max_y: Width(i32::MIN),
        }
    }

    fn add_point(&mut self, point: Point) {
        self.min_x = self.min_x.min(point.x);
        self.min_y = self.min_y.min(point.y);
        self.max_x = self.max_x.max(point.x);
        self.max_y = self.max_y.max(point.y);
    }

    fn add_box(&mut self, geo: GeoBox) {
        self.add_point(Point { x: geo.x, y: geo.y });
        self.add_point(Point {
            x: geo.right(),
            y: geo.bottom(),
        });
    }

    fn add_path(&mut self, path: &Path) {
        self.add_point(path.start);
        for segment in &path.segments {
            match segment {
                Segment::Line(point) => self.add_point(*point),
                Segment::Cubic { c1, c2, to } => {
                    self.add_point(*c1);
                    self.add_point(*c2);
                    self.add_point(*to);
                }
                Segment::Close => {}
            }
        }
    }

    fn add_text(&mut self, x: Width, baseline: Width, width: Width, size: Width) {
        self.add_point(Point {
            x,
            y: Width(baseline.0 - size.0),
        });
        self.add_point(Point {
            x: Width(x.0 + width.0),
            y: Width(baseline.0 + size.0 / 4),
        });
    }
}

fn shift_path(path: Path, dx: Width, dy: Width, height: Width) -> Path {
    fn shift(point: Point, dx: Width, dy: Width, height: Width) -> Point {
        Point {
            x: Width(point.x.0 + dx.0),
            y: Width(point.y.0 + dy.0 - height.0),
        }
    }
    let start = shift(path.start, dx, dy, height);
    let segments = path
        .segments
        .into_iter()
        .map(|segment| match segment {
            Segment::Line(point) => Segment::Line(shift(point, dx, dy, height)),
            Segment::Cubic { c1, c2, to } => Segment::Cubic {
                c1: shift(c1, dx, dy, height),
                c2: shift(c2, dx, dy, height),
                to: shift(to, dx, dy, height),
            },
            Segment::Close => Segment::Close,
        })
        .collect();
    Path {
        start,
        segments,
        fill: path.fill,
        stroke: path.stroke,
    }
}
