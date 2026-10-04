//! Block processing (port of `blocks/BlockProcessor.scala`).

use std::sync::Arc;

use rchain_block_storage::block_store::BlockStore;
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_models::block_hash::BlockHash;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_shared::log::{Log, LogSource};
use tokio::sync::mpsc;

use crate::block_status::BlockStatus;
use crate::merging::BlockIndex;
use crate::multi_parent_casper::ValidateError;
use crate::protocol::comm_util::CommUtil;
use crate::runtime_manager::RuntimeManager;

/// Cap on the number of blocks validated concurrently (per batch). The receiver only emits
/// dependency-free blocks, so a batch of siblings is mutually independent and replayable in parallel.
fn max_parallel_block_validation() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Validate a block and insert it into the DAG (port of `validateAndAddToDag`).
pub async fn validate_and_add_to_dag<F, Fut>(
    dag: &dyn BlockDagStorage,
    block_store: &BlockStore,
    runtime: &RuntimeManager,
    block: BlockMessage,
    shard_id: &str,
    min_phlo_price: i64,
    max_number_of_parents: i32,
    block_index: &F,
    log: &Arc<dyn Log>,
) -> Result<Result<(), BlockStatus>, String>
where
    F: Fn(BlockHash) -> Fut,
    Fut: std::future::Future<Output = Result<Arc<BlockIndex>, String>>,
{
    let result = crate::multi_parent_casper::validate(
        dag,
        block_store,
        runtime,
        &block,
        shard_id,
        min_phlo_price,
        max_number_of_parents,
        block_index,
        log,
    )
    .await;
    let (block_meta, status) = match result {
        Ok(meta) => (meta, Ok(())),
        Err(ValidateError::ValidationFailed(meta, status)) => (meta, Err(status)),
        Err(ValidateError::SelfEquivocation) => {
            return Err("self-equivocation is a proposer-only outcome".to_string())
        }
        Err(ValidateError::Internal(e)) => return Err(e),
    };
    dag.insert(block_meta, block).await?;
    Ok(status)
}

// **Why nothing here defers a `missing justification` refusal** (AUDIT C211, #223).
//
// This loop used to hold a bounded list of blocks refused for a justification that had not arrived,
// retrying them with the next delivered batch. It was removed on 2026-10-04, after C211's cause was
// fixed at its source, because the refusal it protected against **cannot reach this loop at all**:
//
// * the **only** producer into this processor is `pump_validated_blocks`, fed by the receiver's
//   `out_tx` (`node/src/runtime/node_runtime.rs:852-863`), and `out_tx` is sent a hash **only when
//   that block's dependency set is empty** — `block_receiver.rs`'s `has_all_deps` and `finished`'s
//   release set, both now computed as "every justification is in the DAG", under the receiver's lock
//   (`end_stored_awaiting`, #223's fix). The receiver's *other* outlet, `send_to_validate`'s
//   `put_to_incoming_queue`, goes back into the **receiver** (`incoming_blocks_tx`), not here.
// * the DAG index those two read only ever grows, and `BlockMetadataStore::add` writes the **store
//   first** and the index second (`block_metadata_store.rs:57-70`, AUDIT C172), so
//   `index contains j ⟹ store contains j` for every `j`.
// * `validate::block_number` / `sequence_number` read the **store** (`dag.lookup`), so a released
//   block's every justification is readable and the summary cannot fail this way.
//
// Measured too: the two `fixed-run1`/`fixed-run2` rejoins in `spec/audit/evidence/n223-rejoin-blocks/`
// record **zero** refusals across a full rejoin burst. A deferral here would therefore be dead code —
// and not inert if it ever did fire: a parked block rides **every** subsequent batch, so an
// unresolvable one would be replayed up to the cap on each delivery, for ever.

/// Process incoming blocks: validate a batch concurrently, insert serially in topological order,
/// notify the validated queue, and broadcast the block hash (port of `BlockProcessor.apply`).
pub async fn apply<F, Fut>(
    mut input_blocks: mpsc::Receiver<BlockMessage>,
    validated_tx: mpsc::Sender<BlockMessage>,
    shard_id: String,
    min_phlo_price: i64,
    max_number_of_parents: i32,
    dag: Arc<dyn BlockDagStorage>,
    block_store: BlockStore,
    runtime: Arc<RuntimeManager>,
    comm_util: Arc<CommUtil>,
    block_index: F,
    log: Arc<dyn Log>,
) where
    F: Fn(BlockHash) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Arc<BlockIndex>, String>> + Send + 'static,
{
    let source = LogSource::new("casper.blocks.BlockProcessor");
    while let Some(first) = input_blocks.recv().await {
        // Drain a batch of dependency-free blocks, bounded by the concurrency cap.
        let mut batch = vec![first];
        while batch.len() < max_parallel_block_validation() {
            match input_blocks.try_recv() {
                Ok(block) => batch.push(block),
                Err(_) => break,
            }
        }

        // Validate each block concurrently. Validation is verify-only (replay) and forks its own
        // replay runtime per block, so blocks in the batch are independent.
        let mut handles = Vec::with_capacity(batch.len());
        for block in &batch {
            let dag = dag.clone();
            let block_store = block_store.clone();
            let runtime = runtime.clone();
            let shard_id = shard_id.clone();
            let block_index = block_index.clone();
            let block = block.clone();
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                crate::multi_parent_casper::validate(
                    dag.as_ref(),
                    &block_store,
                    runtime.as_ref(),
                    &block,
                    &shard_id,
                    min_phlo_price,
                    max_number_of_parents,
                    &block_index,
                    &log,
                )
                .await
            }));
        }

        // Insert serially in drained order (a valid topological order: parents are emitted before
        // children), then forward/broadcast only blocks that validated successfully.
        for (block, handle) in batch.into_iter().zip(handles) {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => {
                    log.error(source, &format!("validator task panicked: {e}"));
                    continue;
                }
            };
            let (block_meta, status) = match result {
                Ok(meta) => (meta, Ok(())),
                Err(ValidateError::ValidationFailed(meta, status)) => (meta, Err(status)),
                Err(ValidateError::SelfEquivocation) => {
                    log.error(
                        source,
                        &format!(
                            "Block {} processing error: self-equivocation is a proposer-only outcome",
                            block.block_hash.to_hex()
                        ),
                    );
                    continue;
                }
                Err(ValidateError::Internal(e)) => {
                    // An `Internal` failure is a store or runtime failure this loop has no answer for,
                    // so the block is dropped and the error is logged. **A `missing justification`
                    // cannot arrive here** — see the note on `apply` — so there is nothing to defer.
                    log.error(
                        source,
                        &format!("Block {} processing error: {e}", block.block_hash.to_hex()),
                    );
                    continue;
                }
            };
            if let Err(e) = dag.insert(block_meta, block.clone()).await {
                log.error(
                    source,
                    &format!("Block {} insert error: {e}", block.block_hash.to_hex()),
                );
                continue;
            }
            match status {
                Ok(()) => {
                    // **Awaiting, so a full queue backpressures this processor instead of growing**
                    // (C175). The queue is what the block receiver consumes to advance the round/fringe
                    // state, and the producer here is CPU-bound replay validation — exactly the shape R15
                    // bounded on the other half of this pipeline. `let _ =` stays: a send fails only when
                    // every receiver is gone, which is shutdown, and the block is already in the DAG.
                    let _ = validated_tx.send(block.clone()).await;
                    comm_util
                        .send_block_hash(&block.block_hash, block.sender.as_bytes())
                        .await;
                }
                // **The `Display` sentence, not the `Debug` variant name** (#139). `{status:?}` printed
                // `InvalidStateHash` and nothing else, so an operator could not tell a pre-state
                // disagreement from a post-state one — the two are now different statuses — nor read
                // what either means. The hash-carrying line is the interpreter's
                // (`casper.interpreter.validate`); this one says which block and who sent it.
                Err(status) => log.warn(
                    source,
                    &format!(
                        "Block #{} {} from {} failed validation: {status}",
                        block.block_number,
                        block.block_hash.to_hex(),
                        rchain_shared::base16::encode(&block.sender.as_bytes()[..8])
                    ),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The concurrency cap is never zero: a zero would mean no block is ever validated concurrently —
    /// or, if it were used as a batch size, that a batch of blocks is never processed at all. The
    /// host fallback is the only arm that can go wrong here, and it cannot return zero.
    #[test]
    fn the_parallel_validation_cap_is_never_zero() {
        let cap = max_parallel_block_validation();
        assert!(cap >= 1, "a zero cap would stall block validation: {cap}");
        assert_eq!(
            cap,
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            "the cap follows the host's parallelism"
        );
    }
}
