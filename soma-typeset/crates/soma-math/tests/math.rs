//! Math subset: parsing, integer boxes, spacing, errors.

use soma_core::Width;
use soma_math::{lay_out, layout, parse, MathError, MathMetrics, MathStyle};

struct TestMetrics;

impl MathMetrics for TestMetrics {
    fn advance(&self, text: &str, size: Width) -> Width {
        // One char is half an em; enough to make widths observable.
        Width(text.chars().count() as i32 * size.0 / 2)
    }
    fn ascent(&self, size: Width) -> Width {
        Width(size.0 * 3 / 4)
    }
    fn descent(&self, size: Width) -> Width {
        Width(size.0 / 4)
    }
}

fn formula(source: &str) -> soma_math::MathLayout {
    layout(source, &MathStyle::default(), &TestMetrics).expect("layout")
}

#[test]
fn superscripts_are_smaller_and_above_the_baseline() {
    let result = formula("x^2");
    assert_eq!(result.items.len(), 2);
    let base = &result.items[0];
    let sup = &result.items[1];
    assert_eq!(base.text, "x");
    assert_eq!(sup.text, "2");
    assert!(sup.size.0 < base.size.0, "script size must shrink");
    assert!(sup.y.0 < 0, "a superscript sits above the baseline");
    assert!(sup.x.0 >= base.x.0 + TestMetrics.advance("x", base.size).0);
    assert!(result.width.0 > TestMetrics.advance("x", base.size).0);
}

#[test]
fn a_superscript_is_lifted_by_its_depth_not_its_height() {
    // `mc^2` used to raise the `2` by its own full height, which floats it
    // halfway to the cap line. The lift is `sup_shift` unless the script's
    // depth would bring its bottom closer than the gap.
    let result = formula("x^2");
    let base = &result.items[0];
    let sup = &result.items[1];
    let style = MathStyle::default();
    let shift = base.size.0 as i64 * i64::from(style.sup_shift) / 1000;
    let depth = sup.size.0 as i64 / 4;
    let gap = base.size.0 as i64 * 80 / 4096;
    let expected = shift.max(depth + gap);
    assert_eq!(i64::from(-sup.y.0), expected);
}

#[test]
fn subscripts_sit_below_the_baseline() {
    let result = formula("x_1");
    let sub = result.items.last().expect("subscript");
    assert_eq!(sub.text, "1");
    assert!(sub.size.0 < result.items[0].size.0);
    assert!(sub.y.0 > 0, "a subscript sits below the baseline");
}

#[test]
fn fractions_stack_and_draw_a_rule() {
    let result = formula(r"\frac{a}{b}");
    assert_eq!(result.rules.len(), 1);
    let rule = &result.rules[0];
    assert!(rule.thickness.0 > 0);
    assert!(
        rule.y.0 < 0,
        "the bar sits at the math axis, above the baseline"
    );
    assert!(rule.width.0 > 0);
    assert_eq!(result.items.len(), 2);
    let numerator = &result.items[0];
    let denominator = &result.items[1];
    assert!(numerator.y.0 < rule.y.0, "numerator above the bar");
    assert!(denominator.y.0 > rule.y.0, "denominator below the bar");
    assert!(result.total_height().0 > rule.thickness.0);
}

#[test]
fn binary_and_relation_gaps_widen_the_box() {
    let plain = formula("ab");
    let binary = formula("a+b");
    let relation = formula("a=b");
    assert!(binary.width.0 > plain.width.0);
    assert!(relation.width.0 > binary.width.0);
}

#[test]
fn text_groups_stay_upright_and_full_size() {
    let result = formula(r"\text{中文}");
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].text, "中文");
    assert_eq!(result.items[0].size, Width::EM);
}

#[test]
fn greek_and_operators_are_mapped() {
    let result = formula(r"\alpha \le \beta");
    let text: Vec<&str> = result.items.iter().map(|item| item.text.as_str()).collect();
    assert_eq!(text, vec!["α", "≤", "β"]);
}

#[test]
fn unknown_commands_are_errors() {
    assert_eq!(
        parse(r"\unknown"),
        Err(MathError::UnsupportedCommand(String::from("unknown")))
    );
    assert_eq!(parse("{x"), Err(MathError::UnbalancedGroup));
    assert_eq!(parse(""), Err(MathError::Empty));
}

#[test]
fn layout_is_deterministic_and_parse_is_reusable() {
    let nodes = parse(r"x^2 + \frac{1}{2}").expect("parse");
    let style = MathStyle::default();
    let first = lay_out(&nodes, style.size, &style, &TestMetrics);
    let second = lay_out(&nodes, style.size, &style, &TestMetrics);
    assert_eq!(first, second);
    assert!(!first.items.is_empty());
}

#[test]
fn a_taller_script_expands_the_box() {
    let plain = formula("x");
    let with_sup = formula(r"x^{\frac{1}{2}}");
    assert!(with_sup.total_height().0 > plain.total_height().0);
    assert!(with_sup.width.0 >= plain.width.0);
}

#[test]
fn a_radical_draws_a_bar_over_its_body() {
    let result = formula(r"\sqrt{x+1}");

    let glyph = result
        .items
        .iter()
        .find(|item| item.text == "√")
        .expect("radical glyph");
    assert_eq!(result.rules.len(), 1);
    let bar = &result.rules[0];
    assert!(bar.width.0 > 0 && bar.thickness.0 > 0);
    assert!(
        bar.x.0 >= glyph.x.0 + TestMetrics.advance("√", glyph.size).0 - 1,
        "the bar starts after the radical glyph"
    );
    // The bar sits at the top of the body and the glyph is stretched.
    assert!(bar.y.0 <= -TestMetrics.ascent(glyph.size).0 + bar.thickness.0);
    assert!(glyph.size.0 >= Width::EM.0, "the radical covers the body");
    assert!(result.total_height().0 >= bar.thickness.0);
}

#[test]
fn a_radical_degree_sits_before_the_glyph() {
    let plain = formula(r"\sqrt{x}");
    let cubed = formula(r"\sqrt[3]{x}");

    assert!(cubed.width.0 > plain.width.0, "the degree adds width");
    let degree = cubed
        .items
        .iter()
        .find(|item| item.text == "3")
        .expect("degree");
    let glyph = cubed
        .items
        .iter()
        .find(|item| item.text == "√")
        .expect("radical glyph");
    assert!(degree.x.0 < glyph.x.0);
    assert!(degree.y.0 < 0, "the degree sits above the baseline");
}

#[test]
fn a_sum_stacks_its_limits_around_the_operator() {
    let result = formula(r"\sum_{i=1}^{n} i");

    let operator = result
        .items
        .iter()
        .find(|item| item.text == "∑")
        .expect("operator");
    assert!(operator.size.0 > Width::EM.0, "big operators grow");

    let upper = result
        .items
        .iter()
        .find(|item| item.text == "n")
        .expect("upper limit");
    let lower = result
        .items
        .iter()
        .find(|item| item.text == "i" && item.y.0 > 0)
        .expect("lower limit");

    assert!(upper.y.0 < operator.y.0, "the upper limit is above");
    assert!(lower.y.0 > operator.y.0, "the lower limit is below");
    assert!(
        upper.y.0 <= -TestMetrics.ascent(operator.size).0 + TestMetrics.descent(upper.size).0,
        "the upper limit clears the operator"
    );
    assert!(
        lower.y.0 >= TestMetrics.descent(operator.size).0,
        "the lower limit clears the operator"
    );
}

#[test]
fn an_integral_keeps_its_limits_to_the_right() {
    let result = formula(r"\int_0^1 x");
    let operator = result
        .items
        .iter()
        .find(|item| item.text == "∫")
        .expect("operator");
    let upper = result
        .items
        .iter()
        .find(|item| item.text == "1")
        .expect("upper limit");

    assert!(upper.x.0 >= operator.x.0 + TestMetrics.advance("∫", operator.size).0);
    assert!(upper.y.0 < 0);
}

#[test]
fn function_names_stay_upright_words() {
    let result = formula(r"\log x + \sin y");
    let text: Vec<&str> = result.items.iter().map(|item| item.text.as_str()).collect();

    assert_eq!(text, vec!["log", "x", "+", "sin", "y"]);
}

#[test]
fn left_and_right_delimiters_are_kept() {
    let result = formula(r"\left( \frac{a}{b} \right)");
    let text: Vec<&str> = result.items.iter().map(|item| item.text.as_str()).collect();

    assert_eq!(text.first().copied(), Some("("));
    assert_eq!(text.last().copied(), Some(")"));
}

#[test]
fn a_null_delimiter_draws_nothing() {
    let result = formula(r"\left. x \right. ");
    let text: Vec<&str> = result.items.iter().map(|item| item.text.as_str()).collect();

    assert_eq!(text, vec!["x"]);
}

#[test]
fn an_unknown_big_operator_still_errors() {
    assert!(matches!(
        parse(r"\bigcup"),
        Err(MathError::UnsupportedCommand(_))
    ));
}

#[test]
fn inline_style_keeps_big_operator_limits_to_the_right() {
    let inline = MathStyle {
        display: false,
        ..MathStyle::default()
    };
    let nodes = parse(r"\sum_{i=1}^{n} i").expect("parse");
    let result = lay_out(&nodes, inline.size, &inline, &TestMetrics);

    let operator = result
        .items
        .iter()
        .find(|item| item.text == "∑")
        .expect("operator");
    let upper = result
        .items
        .iter()
        .find(|item| item.text == "n")
        .expect("upper limit");
    let lower = result
        .items
        .iter()
        .find(|item| item.text == "i" && item.y.0 > 0)
        .expect("lower limit");

    assert!(
        upper.x.0 >= operator.x.0 + TestMetrics.advance("∑", operator.size).0,
        "inline limits follow the operator"
    );
    assert!(lower.x.0 >= operator.x.0 + TestMetrics.advance("∑", operator.size).0);
    assert!(upper.y.0 < operator.y.0, "the upper limit is raised");
    assert!(lower.y.0 > operator.y.0, "the lower limit is dropped");
    assert!(result.total_height().0 < formula(r"\sum_{i=1}^{n} i").total_height().0);
}

#[test]
fn an_integral_shrinks_but_keeps_its_limits_inline() {
    let inline = MathStyle {
        display: false,
        ..MathStyle::default()
    };
    let nodes = parse(r"\int_0^1 x").expect("parse");
    let inline_result = lay_out(&nodes, inline.size, &inline, &TestMetrics);
    let display_result = lay_out(&nodes, inline.size, &MathStyle::default(), &TestMetrics);

    let inline_int = inline_result
        .items
        .iter()
        .find(|item| item.text == "∫")
        .expect("operator");
    let display_int = display_result
        .items
        .iter()
        .find(|item| item.text == "∫")
        .expect("operator");

    assert!(
        inline_int.size.0 < display_int.size.0,
        "text style is smaller"
    );
    assert!(inline_result.width.0 < display_result.width.0);
    let upper_inline = inline_result
        .items
        .iter()
        .find(|item| item.text == "1")
        .expect("limit");
    assert!(
        upper_inline.x.0 >= inline_int.x.0 + TestMetrics.advance("∫", inline_int.size).0,
        "limits stay to the right in both styles"
    );
}

#[test]
fn display_fractions_use_full_size_parts_and_text_style_shrinks_them() {
    let display = formula(r"\frac{a}{b}");
    let inline_style = MathStyle {
        display: false,
        ..MathStyle::default()
    };
    let nodes = parse(r"\frac{a}{b}").expect("parse");
    let inline = lay_out(&nodes, inline_style.size, &inline_style, &TestMetrics);

    assert_eq!(
        display.items[0].size,
        Width::EM,
        "display parts are full size"
    );
    assert_eq!(
        inline.items[0].size,
        Width(Width::EM.0 * 700 / 1000),
        "inline parts are script size"
    );
    assert!(inline.width.0 < display.width.0);
}

#[test]
fn a_delimited_group_stretches_its_fences() {
    let result = formula(r"\left( \frac{a}{b} \right)");
    let parens: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.text == "(" || item.text == ")")
        .collect();
    assert_eq!(parens.len(), 2, "one fence on each side");
    assert!(
        parens[0].size.0 > Width::EM.0,
        "the fences grow with the body"
    );
    assert_eq!(parens[0].size, parens[1].size);
    assert!(parens[0].x.0 < parens[1].x.0);

    // The fence box covers the fraction: no gap between the body and the
    // fence on either side.
    let body_top = result
        .items
        .iter()
        .filter(|item| item.text != "(" && item.text != ")")
        .map(|item| -item.y.0 + TestMetrics.ascent(item.size).0)
        .max()
        .expect("body");
    let fence_top = -parens[0].y.0 + TestMetrics.ascent(parens[0].size).0;
    assert!(fence_top >= body_top, "{fence_top} >= {body_top}");
}

#[test]
fn left_and_right_without_a_tall_body_stay_at_base_size() {
    let result = formula(r"\left( x \right)");
    let paren = result
        .items
        .iter()
        .find(|item| item.text == "(")
        .expect("fence");
    assert_eq!(paren.size, Width::EM);
    assert_eq!(paren.y, Width::ZERO, "centred on the baseline");
}

#[test]
fn a_matrix_aligns_columns_and_rows() {
    let result = formula(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
    let find = |text: &str, min_y: i32| {
        result
            .items
            .iter()
            .find(|item| item.text == text && item.y.0 >= min_y)
            .unwrap_or_else(|| panic!("cell {text} at y>={min_y}"))
            .clone()
    };

    let a = find("a", i32::MIN);
    let b = find("b", i32::MIN);
    let c = find("c", i32::MIN);
    let d = find("d", i32::MIN);

    assert!(a.x.0 < b.x.0 && c.x.0 < d.x.0, "columns left to right");
    assert!(a.y.0 < c.y.0 && b.y.0 < d.y.0, "second row is below");
    assert_eq!(a.x, c.x, "cells in one column share an x");
    assert_eq!(b.x, d.x);
    assert_eq!(a.y, b.y, "cells in one row share a baseline");

    let fences: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.text == "(" || item.text == ")")
        .collect();
    assert_eq!(fences.len(), 2);
    assert!(fences[0].x.0 < a.x.0 && fences[1].x.0 > d.x.0);
}

#[test]
fn an_aligned_block_aligns_on_the_ampersand() {
    let result = formula(r"\begin{aligned} \text{x} &= 1 \\ \text{yy} &= 2 \end{aligned}");
    let equals: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.text == "=")
        .collect();
    assert_eq!(equals.len(), 2);
    assert_eq!(equals[0].x, equals[1].x, "the alignment column is shared");

    let left_cell = |y: i32| {
        result
            .items
            .iter()
            .find(|item| (item.text == "x" || item.text == "yy") && item.y.0 == y)
            .expect("left cell")
    };
    let first = left_cell(equals[0].y.0);
    let second = left_cell(equals[1].y.0);

    assert_eq!(first.text, "x");
    assert_eq!(second.text, "yy");
    assert!(
        first.x.0 + TestMetrics.advance("x", first.size).0 <= equals[0].x.0,
        "the left column hugs the alignment point"
    );
    assert!(second.x.0 + TestMetrics.advance("yy", second.size).0 <= equals[1].x.0);
    assert!(
        second.x.0 < first.x.0,
        "right-aligned cells share a right edge"
    );
    assert!(result.width.0 > Width::ZERO.0);
}

#[test]
fn a_cases_block_draws_one_brace() {
    let result = formula(r"\begin{cases} x & x > 0 \\ -x & x \le 0 \end{cases}");
    let braces: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.text == "{")
        .collect();
    assert_eq!(braces.len(), 1, "cases has only a left brace");
    assert!(braces[0].size.0 >= Width::EM.0);

    let top = result
        .items
        .iter()
        .find(|item| item.text == "x")
        .expect("first row");
    let bottom = result
        .items
        .iter()
        .rfind(|item| item.text == "x")
        .expect("last row");
    assert!(bottom.y.0 > top.y.0);
}

#[test]
fn an_unbalanced_environment_is_an_error() {
    assert!(matches!(
        parse(r"\begin{matrix} a & b"),
        Err(MathError::UnbalancedEnvironment(_))
    ));
    assert!(matches!(
        parse(r"\begin{matrix} a \end{pmatrix}"),
        Err(MathError::UnbalancedEnvironment(_))
    ));
    assert!(matches!(
        parse(r"\begin{unknown} a \end{unknown}"),
        Err(MathError::UnsupportedCommand(_))
    ));
    assert!(matches!(
        parse(r"\left( x"),
        Err(MathError::UnbalancedDelimiter)
    ));
}

#[test]
fn a_radical_inside_a_fraction_keeps_its_own_baseline() {
    // `\frac{\sqrt{\pi}}{2}`: the radical used to ignore the baseline its
    // parent gave it, so the numerator collapsed onto the bar. Every numerator
    // glyph must sit above the fraction rule, every denominator glyph below.
    let result = formula("\\frac{\\sqrt{\\pi}}{2}");
    let rules = &result.rules;
    assert_eq!(rules.len(), 2, "the radical bar and the fraction bar");
    let fraction = rules
        .iter()
        .max_by_key(|rule| rule.width.0)
        .expect("fraction rule");
    let radical = rules
        .iter()
        .min_by_key(|rule| rule.width.0)
        .expect("radical bar");

    let pi = result
        .items
        .iter()
        .find(|item| item.text == "π")
        .expect("radicand");
    let two = result
        .items
        .iter()
        .find(|item| item.text == "2")
        .expect("denominator");

    assert!(
        radical.y.0 < pi.y.0,
        "the radical bar sits above its radicand ({} vs {})",
        radical.y.0,
        pi.y.0
    );
    assert!(
        pi.y.0 < fraction.y.0,
        "the numerator sits above the fraction bar ({} vs {})",
        pi.y.0,
        fraction.y.0
    );
    assert!(
        two.y.0 > fraction.y.0,
        "the denominator sits below the fraction bar ({} vs {})",
        two.y.0,
        fraction.y.0
    );
}
