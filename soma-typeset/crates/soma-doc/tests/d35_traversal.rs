//! D35: the shared and mutable DocIR traversals are generated from one
//! structure list, so they must visit the same nodes in the same order.

mod common;

use common::maximal_document_unrenumbered;
use soma_doc::NodeId;

#[test]
fn shared_and_mutable_traversals_visit_the_same_nodes() {
    let mut document = maximal_document_unrenumbered();

    // `node_ids()` runs the shared walk; `renumber()` runs the mutable walk.
    let before = document.node_ids();
    assert!(before.len() > 40, "fixture must cover every node kind");
    assert!(
        before.iter().all(|id| !id.is_assigned()),
        "the unrenumbered fixture must start with zero ids"
    );

    // Independent oracle: full canonical output writes exactly one
    // `"span":{"source":` header per node, including table parts and metadata
    // title inlines. It does not depend on either traversal.
    let json = document.canonical_json();
    let node_heads = json.matches("\"span\":{\"source\":").count();
    assert_eq!(before.len(), node_heads, "shared walk missed a node");
    assert_eq!(document.node_count() as usize, node_heads);

    document.renumber();
    let after = document.node_ids();

    assert_eq!(after.len(), node_heads, "mutable walk changed the node set");
    assert!(
        before
            .iter()
            .zip(&after)
            .all(|(old, new)| old != new && new.is_assigned()),
        "the mutable walk must assign an id to every node"
    );
    let expected: Vec<NodeId> = (1..=after.len() as u32).map(NodeId::new).collect();
    assert_eq!(
        after, expected,
        "ids must be contiguous and in shared-walk order"
    );

    // The writer's own node count is the oracle again after renumbering.
    assert_eq!(document.node_count() as usize, node_heads);
    assert!(document
        .canonical_json()
        .contains(&format!("\"nodeCount\":{node_heads}")));
}

#[test]
fn traversal_order_is_stable_and_renumber_is_idempotent() {
    // The maximal document exercises every container shape through the same
    // generated walk: meta.title, blocks, list items, figure image, table
    // parts, section title, generated blocks, and footnotes.
    let mut document = maximal_document_unrenumbered();
    document.renumber();

    let ids = document.node_ids();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ids, "node ids are assigned in traversal order");

    let first = document.node_ids();
    document.renumber();
    assert_eq!(first, document.node_ids(), "renumber is idempotent");
}
