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
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_shared::log::{Log, LogSource};
use rchain_shared::refined::BlockHeight;

use crate::interpreter_util::ResetRequest;
use crate::runtime_manager::RuntimeManager;

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

/// **What a running node does when the reset waker fires** (#320) — the running loop's reset arm,
/// extracted so the decision and its effect are testable without a transport, an importer, a
/// `ShardSpec` or a `NodeRunning`.
///
/// Three outcomes, and the middle one is the one an operator will meet first:
///
/// * **nothing pending** (`Ok(None)`) — a spurious wake, or a request another arm already took. The
///   running phase carries on; it does *not* rebuild anything;
/// * **the switch is off** (`Ok(None)`, after logging) — the detector has already counted and logged
///   the divergence, and this line is the operator's evidence that the node *would* have reset itself.
///   That is what makes turning `casper.self-reset` on a decision rather than a leap, and it is why the
///   request is consumed here rather than left pending: a surface that showed a request for ever would
///   be the same silence in a different place;
/// * **acted** (`Ok(Some(request))`) — the durable half ran and the caller has an anchor to catch up
///   from.
///
/// **The caller propagates the error rather than continuing**: a rewind that failed leaves a store
/// rewound under a view that was not, and a node in that shape serving on is worse than the refusal the
/// reset replaces.
pub async fn act_on_reset_request(
    self_reset: bool,
    dag: &Arc<dyn BlockDagStorage>,
    block_store: &BlockStore,
    runtime_manager: &Arc<RuntimeManager>,
    log: &Arc<dyn Log>,
    source: LogSource,
) -> Result<Option<ResetRequest>, String> {
    let Some(request) = crate::interpreter_util::take_reset_request() else {
        return Ok(None);
    };
    if !self_reset {
        log.warn(
            source,
            &format!(
                "a reset was requested (rewind to {} at height {}) but `casper.self-reset` is off, so \
                 this node keeps refusing {} and its descendants. Turn it on to let the node reset \
                 itself (#320)",
                request.anchor.to_hex(),
                i64::from(request.anchor_height),
                request.refused.to_hex(),
            ),
        );
        return Ok(None);
    }
    perform_reset(dag, block_store, runtime_manager, &request, log, source).await?;
    Ok(Some(request))
}

/// The act itself, in the order the design requires — see [`rewind_and_purge`] for why the anchor's
/// state is read first and why the two stores move together.
async fn perform_reset(
    dag: &Arc<dyn BlockDagStorage>,
    block_store: &BlockStore,
    runtime_manager: &Arc<RuntimeManager>,
    request: &ResetRequest,
    log: &Arc<dyn Log>,
    source: LogSource,
) -> Result<(), String> {
    let anchor_block = block_store
        .get(&[request.anchor])
        .await?
        .pop()
        .flatten()
        .ok_or_else(|| {
            format!(
                "the reset's anchor {} is not in this node's block store, so the state to reset the \
                 runtime to cannot be read — refusing the reset rather than rewinding to a block this \
                 node cannot replay from",
                request.anchor.to_hex()
            )
        })?;
    let (rewound, removed) = rewind_and_purge(dag, block_store, request.anchor_height).await?;
    runtime_manager
        .runtime()
        .reset(Blake2b256Hash::from_byte_array(
            anchor_block.post_state_hash.as_bytes(),
        ))
        .await?;
    log.warn(
        source,
        &format!(
            "rewound {rewound} block(s) above height {} — {removed} body(ies) removed from the block \
             store so they cannot be served or re-inserted — and reset the runtime to that state (#320)",
            i64::from(request.anchor_height),
        ),
    );
    Ok(())
}
