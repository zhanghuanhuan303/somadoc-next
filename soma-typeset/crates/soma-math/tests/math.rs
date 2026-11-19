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
