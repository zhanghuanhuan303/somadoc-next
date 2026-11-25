//! Shape and arrow geometry. Every shape lowers to the same `Path` primitives
//! TikZ already renders (lines, cubics, close, fill, stroke, dash).

use alloc::vec;
use alloc::vec::Vec;

use soma_core::Width;
use soma_tikz::{Path, Point, Segment, Stroke};

use crate::ir::Shape;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Box {
    pub x: Width,
    pub y: Width,
    pub w: Width,
    pub h: Width,
}

impl Box {
    pub(crate) fn center(self) -> Point {
        Point {
            x: Width(self.x.0 + self.w.0 / 2),
            y: Width(self.y.0 + self.h.0 / 2),
        }
    }

    pub(crate) fn right(self) -> Width {
        Width(self.x.0 + self.w.0)
    }

    pub(crate) fn bottom(self) -> Width {
        Width(self.y.0 + self.h.0)
    }
}

pub(crate) struct Look {
    pub fill: Option<u32>,
    pub stroke: Option<u32>,
    pub text: u32,
    pub width: Width,
    pub dashed: bool,
}

impl Look {
    pub(crate) fn stroke(&self) -> Option<Stroke> {
        self.stroke.map(|color| Stroke {
            color,
            width: self.width,
            dash: if self.dashed {
                let on = self.width.scale(15, 2);
                let off = self.width.scale(15, 2);
                vec![on, off]
            } else {
                Vec::new()
            },
        })
    }
}

fn point(x: Width, y: Width) -> Point {
    Point { x, y }
}

pub(crate) fn shape_paths(shape: Shape, r: Box, look: &Look, radius: Width) -> Vec<Path> {
    let stroke = look.stroke();
    let fill = look.fill;
    match shape {
        Shape::Rect => vec![rect(r, Width::ZERO, fill, stroke.clone())],
        Shape::Round => vec![rect(r, radius, fill, stroke.clone())],
        Shape::Stadium => vec![rect(r, r.h.scale(1, 2), fill, stroke.clone())],
        Shape::Subroutine => {
            let mut paths = vec![rect(r, Width::ZERO, fill, stroke.clone())];
            let inset = radius.max(r.w.scale(1, 10));
            for x in [Width(r.x.0 + inset.0), Width(r.right().0 - inset.0)] {
                paths.push(path(
                    point(x, r.y),
                    vec![Segment::Line(point(x, r.bottom()))],
                    None,
                    stroke.clone(),
                ));
            }
            paths
        }
        Shape::Cylinder => cylinder(r, fill, stroke.clone(), radius),
        Shape::Circle => {
            let radius = Width(r.w.0.min(r.h.0) / 2);
            vec![ellipse(r.center(), radius, radius, fill, stroke.clone())]
        }
        Shape::DoubleCircle => {
            let outer = Width(r.w.0.min(r.h.0) / 2);
            let inner = Width(outer.0 * 9 / 10);
            vec![
                ellipse(r.center(), outer, outer, fill, stroke.clone()),
                ellipse(r.center(), inner, inner, None, stroke.clone()),
            ]
        }
        Shape::Ellipse => {
            let rx = Width(r.w.0 / 2);
            let ry = Width(r.h.0 / 2);
            vec![ellipse(r.center(), rx, ry, fill, stroke.clone())]
        }
        Shape::Diamond => {
            let c = r.center();
            vec![polygon(
                &[
                    point(Width(c.x.0), r.y),
                    point(r.right(), Width(c.y.0)),
                    point(Width(c.x.0), r.bottom()),
                    point(r.x, Width(c.y.0)),
                ],
                fill,
                stroke.clone(),
            )]
        }
        Shape::Hexagon => {
            let inset = Width(r.w.0 / 5);
            let cy = Width(r.y.0 + r.h.0 / 2);
            vec![polygon(
                &[
                    point(Width(r.x.0 + inset.0), r.y),
                    point(Width(r.right().0 - inset.0), r.y),
                    point(r.right(), cy),
                    point(Width(r.right().0 - inset.0), r.bottom()),
                    point(Width(r.x.0 + inset.0), r.bottom()),
                    point(r.x, cy),
                ],
                fill,
                stroke.clone(),
            )]
        }
        Shape::Parallelogram | Shape::ParallelogramAlt => {
            let skew = Width(r.w.0 / 6);
            let points = if shape == Shape::Parallelogram {
                [
                    point(Width(r.x.0 + skew.0), r.y),
                    point(r.right(), r.y),
                    point(Width(r.right().0 - skew.0), r.bottom()),
                    point(r.x, r.bottom()),
                ]
            } else {
                [
                    point(r.x, r.y),
                    point(Width(r.right().0 - skew.0), r.y),
                    point(r.right(), r.bottom()),
                    point(Width(r.x.0 + skew.0), r.bottom()),
                ]
            };
            vec![polygon(&points, fill, stroke.clone())]
        }
        Shape::Trapezoid | Shape::TrapezoidAlt => {
            let inset = Width(r.w.0 / 6);
            let points = if shape == Shape::Trapezoid {
                [
                    point(Width(r.x.0 + inset.0), r.y),
                    point(Width(r.right().0 - inset.0), r.y),
                    point(r.right(), r.bottom()),
                    point(r.x, r.bottom()),
                ]
            } else {
                [
                    point(r.x, r.y),
                    point(r.right(), r.y),
                    point(Width(r.right().0 - inset.0), r.bottom()),
                    point(Width(r.x.0 + inset.0), r.bottom()),
                ]
            };
            vec![polygon(&points, fill, stroke.clone())]
        }
        Shape::Asymmetric => {
            let point_size = Width(r.w.0 / 5);
            let cy = Width(r.y.0 + r.h.0 / 2);
            vec![polygon(
                &[
                    point(r.x, r.y),
                    point(Width(r.right().0 - point_size.0), r.y),
                    point(r.right(), cy),
                    point(Width(r.right().0 - point_size.0), r.bottom()),
                    point(r.x, r.bottom()),
                ],
                fill,
                stroke.clone(),
            )]
        }
    }
}

#[allow(clippy::vec_init_then_push)]
pub(crate) fn rect(r: Box, radius: Width, fill: Option<u32>, stroke: Option<Stroke>) -> Path {
    if radius.0 <= 0 {
        return polygon(
            &[
                point(r.x, r.y),
                point(r.right(), r.y),
                point(r.right(), r.bottom()),
                point(r.x, r.bottom()),
            ],
            fill,
            stroke,
        );
    }
    let radius = radius.min(r.w.scale(1, 2)).min(r.h.scale(1, 2));
    let k = radius.scale(5523, 10000);
    let (l, t, rr, b) = (r.x, r.y, r.right(), r.bottom());
    let mut segments = Vec::new();
    segments.push(Segment::Line(point(Width(l.0 + radius.0), t)));
    segments.push(Segment::Line(point(Width(rr.0 - radius.0), t)));
    segments.push(Segment::Cubic {
        c1: point(Width(rr.0 - radius.0 + k.0), t),
        c2: point(rr, Width(t.0 + radius.0 - k.0)),
        to: point(rr, Width(t.0 + radius.0)),
    });
    segments.push(Segment::Line(point(rr, Width(b.0 - radius.0))));
    segments.push(Segment::Cubic {
        c1: point(rr, Width(b.0 - radius.0 + k.0)),
        c2: point(Width(rr.0 - radius.0 + k.0), b),
        to: point(Width(rr.0 - radius.0), b),
    });
    segments.push(Segment::Line(point(Width(l.0 + radius.0), b)));
    segments.push(Segment::Cubic {
        c1: point(Width(l.0 + radius.0 - k.0), b),
        c2: point(l, Width(b.0 - radius.0 + k.0)),
        to: point(l, Width(b.0 - radius.0)),
    });
    segments.push(Segment::Line(point(l, Width(t.0 + radius.0))));
    segments.push(Segment::Cubic {
        c1: point(l, Width(t.0 + radius.0 - k.0)),
        c2: point(Width(l.0 + radius.0 - k.0), t),
        to: point(Width(l.0 + radius.0), t),
    });
    segments.push(Segment::Close);
    Path {
        start: point(Width(l.0 + radius.0), t),
        segments,
        fill,
        stroke,
    }
}

pub(crate) fn polygon(points: &[Point], fill: Option<u32>, stroke: Option<Stroke>) -> Path {
    let start = points[0];
    let segments = points[1..]
        .iter()
        .map(|p| Segment::Line(*p))
        .chain(core::iter::once(Segment::Close))
        .collect();
    Path {
        start,
        segments,
        fill,
        stroke,
    }
}

pub(crate) fn ellipse(
    center: Point,
    rx: Width,
    ry: Width,
    fill: Option<u32>,
    stroke: Option<Stroke>,
) -> Path {
    let kx = rx.scale(5523, 10000);
    let ky = ry.scale(5523, 10000);
    let start = point(Width(center.x.0 + rx.0), center.y);
    let segments = vec![
        Segment::Cubic {
            c1: point(Width(center.x.0 + rx.0), Width(center.y.0 + ky.0)),
            c2: point(Width(center.x.0 + kx.0), Width(center.y.0 + ry.0)),
            to: point(center.x, Width(center.y.0 + ry.0)),
        },
        Segment::Cubic {
            c1: point(Width(center.x.0 - kx.0), Width(center.y.0 + ry.0)),
            c2: point(Width(center.x.0 - rx.0), Width(center.y.0 + ky.0)),
            to: point(Width(center.x.0 - rx.0), center.y),
        },
        Segment::Cubic {
            c1: point(Width(center.x.0 - rx.0), Width(center.y.0 - ky.0)),
            c2: point(Width(center.x.0 - kx.0), Width(center.y.0 - ry.0)),
            to: point(center.x, Width(center.y.0 - ry.0)),
        },
        Segment::Cubic {
            c1: point(Width(center.x.0 + kx.0), Width(center.y.0 - ry.0)),
            c2: point(Width(center.x.0 + rx.0), Width(center.y.0 - ky.0)),
            to: point(Width(center.x.0 + rx.0), center.y),
        },
        Segment::Close,
    ];
    Path {
        start,
        segments,
        fill,
        stroke,
    }
}

fn cylinder(r: Box, fill: Option<u32>, stroke: Option<Stroke>, radius: Width) -> Vec<Path> {
    let cap = radius.max(r.h.scale(1, 10));
    let ry = Width(cap.0.min(r.h.0 / 4));
    let rx = Width(r.w.0 / 2);
    let left = r.x;
    let right = r.right();
    let top = Width(r.y.0 + ry.0);
    let bottom = Width(r.bottom().0 - ry.0);
    let start = point(left, top);
    let mut segments = Vec::new();
    // Bottom half is drawn as a cubic between the two sides; the body is a
    // single closed outline.
    let kx = rx.scale(5523, 10000);
    let ky = ry.scale(5523, 10000);
    segments.push(Segment::Line(point(right, top)));
    segments.push(Segment::Line(point(right, bottom)));
    segments.push(Segment::Cubic {
        c1: point(right, Width(bottom.0 + ky.0)),
        c2: point(Width(r.x.0 + rx.0 + kx.0), Width(bottom.0 + ry.0)),
        to: point(Width(r.x.0 + rx.0), Width(bottom.0 + ry.0)),
    });
    segments.push(Segment::Cubic {
        c1: point(Width(r.x.0 + rx.0 - kx.0), Width(bottom.0 + ry.0)),
        c2: point(left, Width(bottom.0 + ky.0)),
        to: point(left, bottom),
    });
    segments.push(Segment::Line(point(left, top)));
    segments.push(Segment::Close);
    let body = Path {
        start,
        segments,
        fill,
        stroke: stroke.clone(),
    };
    // The top rim is a separate stroked ellipse; it must be visible over the
    // body fill.
    let rim = ellipse(point(Width(r.x.0 + rx.0), top), rx, ry, None, stroke);
    vec![body, rim]
}

pub(crate) fn arrow(
    tip: Point,
    direction: (i32, i32),
    size: Width,
    color: u32,
    kind: crate::ir::Tip,
) -> Vec<Path> {
    use crate::ir::Tip;
    let width = Width(size.0 * 2 / 5);
    let (dx, dy) = direction;
    let (nx, ny) = (-dy, dx);
    let base = point(Width(tip.x.0 - dx * size.0), Width(tip.y.0 - dy * size.0));
    let stroke = Some(Stroke {
        color,
        width: Width(size.0 / 5),
        dash: Vec::new(),
    });
    match kind {
        Tip::None => Vec::new(),
        Tip::Arrow => {
            let left = point(
                Width(base.x.0 + nx * width.0),
                Width(base.y.0 + ny * width.0),
            );
            let right = point(
                Width(base.x.0 - nx * width.0),
                Width(base.y.0 - ny * width.0),
            );
            vec![polygon(&[tip, left, right], Some(color), None)]
        }
        Tip::Open => {
            let left = point(
                Width(base.x.0 + nx * width.0),
                Width(base.y.0 + ny * width.0),
            );
            let right = point(
                Width(base.x.0 - nx * width.0),
                Width(base.y.0 - ny * width.0),
            );
            let mut first = path(tip, vec![Segment::Line(left)], None, stroke.clone());
            first.segments.push(Segment::Line(tip));
            let mut second = path(tip, vec![Segment::Line(right)], None, stroke);
            second.segments.push(Segment::Line(tip));
            vec![first, second]
        }
        Tip::Circle => {
            let radius = Width(size.0 / 3);
            vec![ellipse(tip, radius, radius, Some(color), None)]
        }
        Tip::Cross => {
            let first = path(
                point(Width(tip.x.0 - width.0), Width(tip.y.0 - width.0)),
                vec![Segment::Line(point(
                    Width(tip.x.0 + width.0),
                    Width(tip.y.0 + width.0),
                ))],
                None,
                stroke.clone(),
            );
            let second = path(
                point(Width(tip.x.0 + width.0), Width(tip.y.0 - width.0)),
                vec![Segment::Line(point(
                    Width(tip.x.0 - width.0),
                    Width(tip.y.0 + width.0),
                ))],
                None,
                stroke,
            );
            vec![first, second]
        }
    }
}

pub(crate) fn path(
    start: Point,
    segments: Vec<Segment>,
    fill: Option<u32>,
    stroke: Option<Stroke>,
) -> Path {
    Path {
        start,
        segments,
        fill,
        stroke,
    }
}
