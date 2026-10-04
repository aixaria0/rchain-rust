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

/// **How many blocks may wait for a justification that has not arrived yet** (AUDIT C211).
///
/// A block refused for `missing justification` is *transiently* unresolvable, not invalid: the
/// justification it names is on its way. Dropping it is permanent for that block, and the receiver
/// cannot pick it up again — `begin_stored` returns `false` for a hash already in
/// `EndStoreBlock`/`PendingValidation`, so `send_to_validate`'s re-send skips exactly the block that
/// needs re-attempting. Observed live (#223): a rejoining validator was capped at the height it died at
/// with two such refusals and no recovery, the node stalled and the block's hash stayed parked in the
/// receiver for ever. These wait for the next delivered block instead; the bound keeps a block that is
/// genuinely unresolvable from being retried unboundedly.
const MAX_PARKED_BLOCKS: usize = 64;

/// The refusal that is *transient*: the block summary could not read a justification this block names.
///
/// The sentence is the one `validate::block_number` / `validate::sequence_number` build and
/// `multi_parent_casper::validate_checks` wraps as `ValidateError::Internal`. It is matched rather than
/// parsed for the missing hash on purpose: the hash is not needed to decide *whether* to retry, only
/// *when*, and the bound supplies that — so nothing here depends on the message's exact spelling
/// beyond the phrase the two sites share.
fn is_missing_justification(message: &str) -> bool {
    message.contains("missing justification ")
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
    // Blocks refused for a justification that has not arrived yet, retried alongside the next delivered
    // block rather than dropped for ever (C211). Bounded, so a genuinely unresolvable block is dropped
    // by the `MAX_PARKED_BLOCKS` check rather than accumulating.
    let mut parked: Vec<BlockMessage> = Vec::new();
    while let Some(first) = input_blocks.recv().await {
        // Drain a batch of dependency-free blocks, bounded by the concurrency cap.
        let mut batch = vec![first];
        while batch.len() < max_parallel_block_validation() {
            match input_blocks.try_recv() {
                Ok(block) => batch.push(block),
                Err(_) => break,
            }
        }
        // **The retry rides on delivery.** A parked block is re-validated with the next batch, because
        // the justification it needs may have arrived with it. This is a bounded, best-effort recovery,
        // not a complete one: a node that receives nothing further re-attempts nothing, which is why the
        // self-triggered re-drive is named as the follow-up rather than assumed here.
        batch.append(&mut parked);

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
                    // **A missing justification is transient, so it is not a drop** (C211). The block
                    // waits for a justification that is on its way; dropping it is permanent, and the
                    // receiver cannot pick it up again (`begin_stored`'s dedupe skips a hash it has
                    // already handed to the validator). Every other `Internal` still drops, because
                    // those are store/runtime failures this loop has no answer for.
                    if is_missing_justification(&e) && parked.len() < MAX_PARKED_BLOCKS {
                        log.warn(
                            source,
                            &format!(
                                "Block {} deferred: a justification it names is not readable yet, so it \
                                 waits for the next delivered block instead of being dropped \
                                 (AUDIT C211): {e}",
                                block.block_hash.to_hex()
                            ),
                        );
                        parked.push(block);
                        continue;
                    }
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

    /// **The deferral predicate, on the exact sentences the two sites build.** C211 turns on telling a
    /// *transient* refusal from a permanent one: `validate::block_number`/`sequence_number` render
    /// `missing justification <hex>`, `validate_checks` wraps that as `block summary failed: {e}`, and the
    /// processor parks on the phrase rather than dropping the block for ever. A predicate that matched
    /// too much would defer store failures; one that matched too little would defer nothing, which is
    /// the defect. Both directions are pinned here.
    #[test]
    fn only_a_missing_justification_is_deferred() {
        // The wrapped sentence the processor actually sees, both renderers.
        assert!(is_missing_justification(
            "block summary failed: missing justification 4e0fb9c6aa"
        ));
        assert!(is_missing_justification(
            "block summary failed: missing justification b31b0600ff"
        ));
        // The other `Internal` shapes must keep dropping: they are store/runtime failures this loop
        // has no answer for, and deferring them would retry a failure that cannot clear.
        assert!(!is_missing_justification(
            "block summary failed: store unavailable"
        ));
        assert!(!is_missing_justification("bondsCache failed: timeout"));
        assert!(!is_missing_justification(
            "validateBlockCheckpoint failed: InvalidStateHash"
        ));
        assert!(!is_missing_justification(""));
        // The phrase alone, with nothing after it, is not the renderer's output.
        assert!(!is_missing_justification("missing justification"));
    }

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
