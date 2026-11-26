//! A2 units: exact conversion, no float, documented truncation.

use soma_core::Width;
use soma_template::{LengthSpec, LineHeightSpec, Rational, Unit};

/// `10.5pt` body text.
fn body() -> Rational {
    Rational::new(21, 2).expect("non-zero denominator")
}

fn parse(text: &str) -> LengthSpec {
    LengthSpec::parse(text).unwrap_or_else(|| panic!("{text} must parse"))
}

#[test]
fn decimals_are_exact_rationals() {
    let value = Rational::parse("10.5").expect("decimal");
    assert_eq!(value.num(), 21);
    assert_eq!(value.den(), 2);

    assert_eq!(Rational::parse("0").unwrap().num(), 0);
    assert_eq!(Rational::parse("-2").unwrap().num(), -2);
    assert_eq!(Rational::parse(".5").unwrap().num(), 1);
    assert_eq!(Rational::parse(".5").unwrap().den(), 2);

    // Exponents and separators are rejected: an exact value must never depend
    // on a float parser.
    assert!(Rational::parse("1e3").is_none());
    assert!(Rational::parse("1,5").is_none());
    assert!(Rational::parse("").is_none());
}

#[test]
fn em_resolves_against_the_current_font_size() {
    assert_eq!(parse("1em").resolve(Width::EM, body()), Width::EM);
    assert_eq!(parse("2em").resolve(Width::EM, body()), Width(8192));
    assert_eq!(parse("1.5em").resolve(Width(7801), body()), Width(11701));
    assert_eq!(parse("0").resolve(Width::EM, body()), Width::ZERO);
}

#[test]
fn points_use_the_body_anchor() {
    // The root font size defines 1em, so 10.5pt at a 10.5pt body is exactly 1em.
    assert_eq!(parse("10.5pt").resolve(Width::EM, body()), Width::EM);
    assert_eq!(parse("21pt").resolve(Width::EM, body()), Width(8192));
}

#[test]
fn millimetres_are_exact() {
    // 1mm = 360/127 pt; the result truncates toward zero at 1/4096 em.
    assert_eq!(parse("20mm").resolve(Width::EM, body()), Width(22115));
    assert_eq!(parse("210mm").resolve(Width::EM, body()), Width(232214));
    assert_eq!(parse("1in").resolve(Width::EM, body()), Width(28086));
}

#[test]
fn truncation_is_toward_zero() {
    let third = LengthSpec::new(Rational::new(1, 3).unwrap(), Unit::Em);
    assert_eq!(third.resolve(Width::EM, body()), Width(1365));

    let negative = LengthSpec::new(Rational::new(-1, 3).unwrap(), Unit::Em);
    assert_eq!(negative.resolve(Width::EM, body()), Width(-1365));
}

#[test]
fn units_are_parsed_strictly() {
    assert!(
        LengthSpec::parse("2").is_none(),
        "bare non-zero is ambiguous"
    );
    assert!(LengthSpec::parse("2px").is_none(), "px is not a print unit");
    assert!(LengthSpec::parse("2 em").is_none());
    assert_eq!(parse("0").unit, Unit::Em);
    assert_eq!(parse("2EM"), parse("2em"));
}

#[test]
fn line_height_multiplier_uses_the_resolved_font_size() {
    let multiplier = LineHeightSpec::Multiple(Rational::new(8, 5).unwrap());
    assert_eq!(multiplier.resolve(Width::EM, body()), Width(6553));

    let fixed = LineHeightSpec::Length(parse("10.5pt"));
    assert_eq!(fixed.resolve(Width::EM, body()), Width::EM);
}

#[test]
fn family_names_select_the_three_roles() {
    use soma_template::{family_is_sans, family_role, FamilyRole};
    assert!(!family_is_sans(None), "no family means serif");
    assert!(!family_is_sans(Some("")));
    assert!(!family_is_sans(Some("Songti SC")));
    assert!(family_is_sans(Some("sans-serif")));
    assert!(family_is_sans(Some("SANS")));
    assert!(family_is_sans(Some("Heiti SC")), "无衬线的中文名");
    assert!(family_is_sans(Some("Source Han Gothic")));
    assert!(family_is_sans(Some("IBM Plex Sans SC")));

    // Monospace is a third role, not a synonym for sans (ADR-0089).
    assert_eq!(family_role(Some("monospace")), FamilyRole::Mono);
    assert_eq!(family_role(Some("IBM Plex Mono")), FamilyRole::Mono);
    assert_eq!(family_role(Some("Source Code Pro")), FamilyRole::Mono);
    assert!(!family_is_sans(Some("monospace")));
    assert_eq!(family_role(None), FamilyRole::Serif);
    assert_eq!(family_role(Some("IBM Plex Sans SC")), FamilyRole::Sans);
}
