//! Reference templates keep their historical canonical bytes and hashes.
//!
//! These values were captured before D42/D43/D44/D47 added syntax. The new
//! syntax is additive: an unused key, field, or unit must not perturb a template
//! that does not use it.

use soma_template::parse_template;

const PLEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/templates/plex.yaml"
));
const BOOK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/book/book.yaml"
));

#[test]
fn reference_templates_keep_their_canonical_hashes() {
    let plex = parse_template(PLEX).expect("plex reference template");
    assert_eq!(plex.template_hash().0, 4_467_990_681_682_398_639);
    assert_eq!(plex.canonical_json().len(), 13_637);

    let book = parse_template(BOOK).expect("book reference template");
    assert_eq!(book.template_hash().0, 17_119_048_038_987_802_348);
    assert_eq!(book.canonical_json().len(), 8_280);
}
