//! Core TikZ geometry (ADR-0092).

use soma_math::MathLayout;
use soma_tikz::{layout, Segment, TikzError};

fn empty(_: &str, _: i64) -> Option<MathLayout> {
    Some(MathLayout::default())
}

fn render(source: &str) -> Result<soma_tikz::TikzLayout, TikzError> {
    layout(source, 6_554, &mut empty)
}

#[test]
fn rectangle_uses_the_default_centimetre_unit() {
    let layout = render("\\begin{tikzpicture}\\draw (0,0) rectangle (2,1);\\end{tikzpicture}")
        .expect("rectangle");
    // 2cm x 1cm at 1/4096 em and 10pt body.
    assert!(
        (layout.width.0 - 23_220).abs() <= 4,
        "width {}",
        layout.width.0
    );
    assert!(
        (layout.height.0 - 11_610).abs() <= 4,
        "height {}",
        layout.height.0
    );
    assert_eq!(layout.paths.len(), 1);
    assert_eq!(layout.paths[0].segments.len(), 4);
    assert!(layout.paths[0].stroke.is_some());
    assert_eq!(layout.paths[0].start.x.0, 0);
    assert_eq!(layout.paths[0].start.y.0, 0);
}

#[test]
fn units_shift_and_scale_apply_to_coordinates() {
    let layout =
        render("\\begin{tikzpicture}[xshift=1cm]\\draw (0,0) -- (1cm,0);\\end{tikzpicture}")
            .expect("shift");
    assert!(layout.width.0 > 0);
    assert_eq!(layout.paths[0].start.x.0, 0);
    let end = layout.paths[0]
        .segments
        .iter()
        .find_map(|segment| match segment {
            Segment::Line(point) => Some(*point),
            _ => None,
        })
        .expect("line");
    assert!(end.x.0 > 0);
}

#[test]
fn circle_and_arc_are_cubic_segments() {
    let circle =
        render("\\begin{tikzpicture}\\draw (0,0) circle (1);\\end{tikzpicture}").expect("circle");
    assert!(!circle.paths.is_empty());
    assert!(matches!(circle.paths[0].segments[0], Segment::Cubic { .. }));
    assert!((circle.width.0 - circle.height.0).abs() <= 4);

    let arc =
        render("\\begin{tikzpicture}\\draw (1,0) arc (0:90:1);\\end{tikzpicture}").expect("arc");
    assert!(!arc.paths.is_empty());
    assert!(arc.height.0 > 0);
}

#[test]
fn named_coordinates_and_anchors_resolve() {
    let layout = render(
        "\\begin{tikzpicture}\\coordinate (A) at (1,1);\\draw (A) -- (2,1);\\end{tikzpicture}",
    )
    .expect("named");
    assert!(!layout.paths.is_empty());
}

#[test]
fn foreach_expands_coordinates() {
    let layout = render(
        "\\begin{tikzpicture}\\foreach \\x in {0,1,2}{\\draw (\\x,0) -- (\\x,1);}\\end{tikzpicture}",
    )
    .expect("foreach");
    assert_eq!(layout.paths.len(), 3);
}

#[test]
fn grid_draws_a_lattice_and_honours_step() {
    let default =
        render("\\begin{tikzpicture}\\draw (0,0) grid (2,1);\\end{tikzpicture}").expect("grid");
    // 3 verticals (0,1,2) + 2 horizontals (0,1).
    assert_eq!(default.paths.len(), 5);
    assert!(default.paths.iter().all(|path| path.stroke.is_some()));

    let stepped =
        render("\\begin{tikzpicture}\\draw[step=0.5] (0,0) grid (1,1);\\end{tikzpicture}")
            .expect("stepped grid");
    // 3 verticals + 3 horizontals with a 0.5cm step over 1cm.
    assert_eq!(stepped.paths.len(), 6);
    assert!((stepped.width.0 - default.width.0 / 2).abs() <= 4);
}

#[test]
fn grid_has_no_area_when_filled() {
    // PGF's grid is a set of line subpaths, so `\fill` draws nothing; matching
    // that is part of the subset contract.
    let filled = render("\\begin{tikzpicture}\\fill[red] (0,0) grid (1,1);\\end{tikzpicture}")
        .expect("filled grid");
    assert!(filled.paths.is_empty());

    let error = render("\\begin{tikzpicture}\\draw[step=0] (0,0) grid (1,1);\\end{tikzpicture}")
        .expect_err("zero step");
    assert!(error.message().contains("positive"), "{error}");
}

#[test]
fn foreach_body_can_be_followed_by_another_statement() {
    let layout = render(
        "\\begin{tikzpicture}\\foreach \\x in {0,1} {\\draw (\\x,0) -- (\\x,1);}\\draw (0,1) -- (1,0);\\end{tikzpicture}",
    )
    .expect("foreach tail");
    assert_eq!(layout.paths.len(), 3);
}

#[test]
fn unsupported_keys_are_errors_not_silent_guesses() {
    let error = render("\\begin{tikzpicture}\\draw[plot=true] (0,0) -- (1,0);\\end{tikzpicture}")
        .expect_err("unsupported option");
    assert!(error.message().contains("unsupported option"), "{error}");
}

#[test]
fn node_text_is_measured_and_placed() {
    fn measure(text: &str, _: i64) -> Option<MathLayout> {
        assert_eq!(text, "x");
        Some(MathLayout {
            width: soma_core::Width::from_em(1),
            height: soma_core::Width::from_em(7),
            depth: soma_core::Width::from_em(2),
            ..MathLayout::default()
        })
    }
    let layout = layout(
        "\\begin{tikzpicture}\\node[above] at (1,0) {x};\\end{tikzpicture}",
        6_554,
        &mut measure,
    )
    .expect("node");
    assert!(layout.height.0 > 0);
    assert!(layout.width.0 > 0);
}

#[test]
fn node_anchors_and_orthogonal_paths() {
    let source = "\\begin{tikzpicture}
  \\node[draw, rounded corners=2pt, fill=blue!10] (A) at (0,0) {Start};
  \\node[draw, circle] (B) at (3,0) {End};
  \\draw[->] (A.east) -- (B.west);
  \\draw (A.south) -- ++(0,-0.8) -| (B.south);
  \\coordinate (C) at (1.5,1);
  \\draw[dashed] (A.north) |- (C) -| (B.north);
\\end{tikzpicture}";
    let result = render(source);
    if let Ok(layout) = &result {
        println!("{} paths, {} items", layout.paths.len(), layout.items.len());
    }
    result.expect("node diagram");
}
