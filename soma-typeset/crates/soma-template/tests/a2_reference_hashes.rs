//! Reference templates keep their canonical bytes and hashes.
//!
//! These values pin the shipped reference templates. Update them when a design
//! change is intentional: template v2 (ADR-0114) finished the Plex design for
//! proposals, so its hash and length moved with it.

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
    assert_eq!(plex.template_hash().0, 2_769_758_430_991_782_734);
    assert_eq!(plex.canonical_json().len(), 15_299);

    let book = parse_template(BOOK).expect("book reference template");
    assert_eq!(book.template_hash().0, 17_119_048_038_987_802_348);
    assert_eq!(book.canonical_json().len(), 8_280);
}
