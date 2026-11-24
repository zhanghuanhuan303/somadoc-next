//! D34: `Date` rejects impossible calendar days, including non-leap
//! February and 30-day months.

use soma_doc::meta::{Date, DocumentMeta};
use soma_doc::Document;

fn document_with_date(date: Date) -> Document {
    let mut document = Document::new(DocumentMeta::new().with_date(date));
    document.renumber();
    document
}

#[test]
fn calendar_dates_are_validated() {
    for date in [
        Date::new(2024, 2, 29),
        Date::new(2000, 2, 29),
        Date::new(2026, 11, 20),
        Date::new(2023, 1, 31),
        Date::new(2024, 4, 30),
        Date::new(1, 1, 1),
        Date::new(9999, 12, 31),
    ] {
        assert!(
            date.is_in_range(),
            "{date:?} must be a valid Gregorian date"
        );
        assert!(
            document_with_date(date).validate().is_ok(),
            "{date:?} must validate"
        );
    }
}

#[test]
fn impossible_days_are_rejected() {
    for date in [
        Date::new(2023, 2, 29),
        Date::new(2024, 4, 31),
        Date::new(2024, 2, 30),
        Date::new(1900, 2, 29),
        Date::new(2100, 2, 29),
        Date::new(2023, 13, 1),
        Date::new(2023, 0, 10),
        Date::new(2023, 1, 0),
        Date::new(2023, 1, 32),
        Date::new(0, 1, 1),
    ] {
        assert!(!date.is_in_range(), "{date:?} must be invalid");
        let error = document_with_date(date)
            .validate()
            .expect_err("invalid dates must not validate");
        assert!(
            error.reason.contains("date is out of range"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn leap_year_rule_is_gregorian() {
    assert!(Date::new(2000, 2, 29).is_in_range());
    assert!(!Date::new(1900, 2, 29).is_in_range());
    assert!(!Date::new(2100, 2, 29).is_in_range());
}

#[test]
fn accepted_dates_keep_their_canonical_bytes() {
    let document = document_with_date(Date::new(2024, 2, 29));
    assert!(document
        .canonical_json()
        .contains("\"date\":{\"year\":2024,\"month\":2,\"day\":29}"));
}
