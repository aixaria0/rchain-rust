//! The reset's durable half — what a node rewinding to its last finalised block must do to *two* stores
//! (#320, F-U5-01's acting half).
//!
//! **Why this is a function of its own.** The reset's other halves are process state (the runtime's root,
//! the catch-up walk) and they live where their handles do; this half is two store operations whose
//! effects outlive the process, and it is the half a test can reach without a `RuntimeManager`. Splitting
//! it out is what turns "the loop looks right" into an assertion about the stores — which is the same
//! moved `drop_above`'s own test makes for the DAG alone ("a rewind that lived only in memory would be
//! undone by the next boot").
//!
//! **And the two operations are one meaning.** `drop_above` rewinds the DAG and deliberately leaves
//! `BlockStore` alone — it does not own it, and its own doc says so. Leaving the bodies is not harmless:
//!
//! * the node keeps **serving** them (`node_running::block_is_known` asks the store, so a peer
//!   requesting a dropped block gets it — a node handing out blocks it can no longer replay);
//! * and its **receiver re-inserts** them. `block_receiver::not_validated` reads a body that is stored
//!   but absent from the DAG as *"stored but not yet validated"*, adds it to `parents_to_validate`, and
//!   `send_to_validate` puts it back on the incoming queue — so the next child of a dropped block
//!   arriving would re-insert the dropped block **above the anchor** and undo the reset entirely.
//!
//! So the bodies go with the metadata, and the rewound state becomes unrepresentable rather than guarded:
//! there is nothing left to serve or to re-insert, and everything above the anchor is re-fetched from
//! peers, which is the whole of what a rewind means.

use std::sync::Arc;

use rchain_block_storage::block_store::BlockStore;
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_shared::refined::BlockHeight;

/// Rewind the DAG to `anchor_height` and remove the bodies of everything it dropped.
///
/// Returns `(blocks rewound, bodies removed)`. **An error is fatal and the caller must not carry on**:
/// `drop_above` deletes the persisted metadata and *then* rebuilds the representation, and the rebuild
/// can refuse (it re-applies the equivocation gate), which leaves a store that has been rewound under a
/// view that has not. A node in that shape is worse than the refusal it replaces, so the caller
/// propagates rather than logging and continuing.
pub async fn rewind_and_purge(
    dag: &Arc<dyn BlockDagStorage>,
    block_store: &BlockStore,
    anchor_height: BlockHeight,
) -> Result<(usize, usize), String> {
    let dropped = dag.drop_above(anchor_height).await.map_err(|e| {
        format!(
            "the reset's rewind failed: {e}. This is fatal on purpose — the store may have been rewound \
             under a view that was not rebuilt, so the node must not keep serving"
        )
    })?;
    let deleted = block_store.delete(&dropped).await?;
    Ok((dropped.len(), deleted))
}
