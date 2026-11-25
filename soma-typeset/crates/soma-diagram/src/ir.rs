//! Diagram IR shared by every frontend and by the layout engine.

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::Width;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    TopBottom,
    BottomTop,
    LeftRight,
    RightLeft,
}

impl Direction {
    pub(crate) fn horizontal(self) -> bool {
        matches!(self, Self::LeftRight | Self::RightLeft)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    Diamond,
    Hexagon,
    Parallelogram,
    ParallelogramAlt,
    Trapezoid,
    TrapezoidAlt,
    Asymmetric,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineStyle {
    Solid,
    Dashed,
    Thick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tip {
    None,
    Arrow,
    Open,
    Circle,
    Cross,
}

/// A visual override. `None` means "inherit"; `no_fill`/`no_stroke` are the
/// explicit `fill:none` / `stroke:none` Mermaid spellings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub fill: Option<u32>,
    pub no_fill: bool,
    pub stroke: Option<u32>,
    pub no_stroke: bool,
    pub text: Option<u32>,
    pub width: Option<Width>,
    pub dashed: Option<bool>,
}

impl Style {
    pub(crate) fn merge(&mut self, other: &Self) {
        if other.fill.is_some() {
            self.fill = other.fill;
            self.no_fill = false;
        }
        if other.no_fill {
            self.no_fill = true;
            self.fill = None;
        }
        if other.stroke.is_some() {
            self.stroke = other.stroke;
            self.no_stroke = false;
        }
        if other.no_stroke {
            self.no_stroke = true;
            self.stroke = None;
        }
        if other.text.is_some() {
            self.text = other.text;
        }
        if other.width.is_some() {
            self.width = other.width;
        }
        if other.dashed.is_some() {
            self.dashed = other.dashed;
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub id: String,
    /// Forced lines from `<br/>`/`\n`; each line is wrapped again if needed.
    pub lines: Vec<String>,
    pub shape: Shape,
    pub class: Option<String>,
    pub style: Style,
    /// Declaration order; the stable tie-break for ordering.
    pub order: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub lines: Vec<String>,
    pub line: LineStyle,
    pub start_tip: Tip,
    pub end_tip: Tip,
    /// Minimum number of ranks the edge spans (`A ---> B` is longer than `A --> B`).
    pub span: u32,
    pub visible: bool,
    pub constraint: bool,
    pub style: Style,
    pub order: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub title: Vec<String>,
    pub members: Vec<usize>,
    pub parent: Option<usize>,
    pub order: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Diagram {
    pub direction: Option<Direction>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    pub classes: alloc::collections::BTreeMap<String, Style>,
}

impl Diagram {
    pub(crate) fn direction(&self) -> Direction {
        self.direction.unwrap_or(Direction::TopBottom)
    }
}
