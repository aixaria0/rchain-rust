//! **The durable disposition of a received block** (HAZOP U19, law candidate
//! `block-disposition-and-isolation`).
//!
//! The live private-chain incident had a state the node could not represent: a block body in the
//! block store, no DAG entry, no lease, no retry and no fault. The receiver's pending set and the
//! processor's drop arms were both in memory, so a restart forgot the question and the body stayed
//! an orphan for ever (RCA §9; F-U19-01/F-U19-05/F-U19-07).
//!
//! This ledger is the durable half. It holds only the **unsettled** states:
//!
//! * [`BlockDisposition::Pending`] — the body is stored and the node still owes it an outcome:
//!   waiting on a parent, queued for validation, or scheduled for a retry. Written by the receiver
//!   as soon as the body is in the store, so a crash cannot lose the obligation.
//! * [`BlockDisposition::Faulted`] — a node fault (a panicked validator, an internal replay error,
//!   a timed-out validation, a failed DAG insert) left the block without an outcome. It carries the
//!   stage, the reason, the first and last attempt and the attempt count, so the retry loop is a
//!   query and the operator sees what happened.
//!
//! **`Accepted` and `Refused` are not stored here.** Both are DAG metadata
//! (`BlockMetadata::validated`/`validation_failed`), which is the durable record of the verdict; a
//! second row would be a second truth. Settling — either outcome — deletes the row, so the ledger
//! is bounded by the number of blocks the node currently owes an outcome, not by chain height.
//!
//! **What this cannot do, said rather than implied:** rows are written only by this build. A body
//! stored by an older binary has no row, and finding it would mean decoding the whole block store at
//! boot — the memory cost the resource work exists to avoid. Such a body is still reported by the
//! boot count warning (`NodeLaunch`), and only an operator reset or a peer's re-delivery recovers it.
//! The one-time legacy scan remains owed (F-U19-07).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rchain_block_storage::block_store::BlockStore;
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_models::block_hash::BlockHash;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_shared::chan;
use rchain_shared::log::{Log, LogSource};
use rchain_shared::refined::BlockHeight;
use rchain_shared::typed_store::{Codec, KeyValueTypedStore};
use tokio::sync::mpsc;

/// Where a pending body is in the pipeline. Diagnostic only: every pending row is equally
/// re-offerable at boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingStage {
    /// The body is in the store and the receiver is resolving dependencies.
    Stored,
    /// The block was released to the processor; validation has not returned.
    Validating,
    /// A faulted block was re-offered by the retry loop.
    Retry,
}

impl PendingStage {
    fn as_byte(self) -> u8 {
        match self {
            PendingStage::Stored => 0,
            PendingStage::Validating => 1,
            PendingStage::Retry => 2,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, String> {
        match byte {
            0 => Ok(PendingStage::Stored),
            1 => Ok(PendingStage::Validating),
            2 => Ok(PendingStage::Retry),
            other => Err(format!("unknown pending stage {other}")),
        }
    }
}

/// The unsettled state of one stored block. See the module docs for why only these two exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockDisposition {
    Pending {
        block_number: BlockHeight,
        stage: PendingStage,
        since_unix_ms: u64,
    },
    Faulted {
        block_number: BlockHeight,
        stage: String,
        reason: String,
        since_unix_ms: u64,
        last_attempt_unix_ms: u64,
        attempts: u32,
    },
}

impl BlockDisposition {
    pub fn block_number(&self) -> BlockHeight {
        match self {
            BlockDisposition::Pending { block_number, .. }
            | BlockDisposition::Faulted { block_number, .. } => *block_number,
        }
    }

    pub fn attempts(&self) -> u32 {
        match self {
            BlockDisposition::Pending { .. } => 0,
            BlockDisposition::Faulted { attempts, .. } => *attempts,
        }
    }

    pub fn stage_description(&self) -> String {
        match self {
            BlockDisposition::Pending { stage, .. } => format!("pending/{stage:?}"),
            BlockDisposition::Faulted { stage, .. } => stage.clone(),
        }
    }
}

/// **The retry budget before a block is quarantined.** A fault that repeats is not transient; after
/// this many attempts the row stays `Faulted`, the pipeline fault stays visible, and only an
/// operator restart/reset or a peer's re-delivery can move it. The number is policy.
pub const MAX_FAULT_RETRIES: u32 = 8;

/// The first retry delay. Doubles per attempt up to [`MAX_RETRY_DELAY_MS`], so the eight attempts
/// span roughly half an hour without hammering a failing store or replay.
pub const FIRST_RETRY_DELAY_MS: u64 = 30_000;

pub const MAX_RETRY_DELAY_MS: u64 = 600_000;

fn retry_delay_ms(attempts: u32) -> u64 {
    let shift = attempts.saturating_sub(1).min(5);
    FIRST_RETRY_DELAY_MS
        .saturating_mul(1u64 << shift)
        .min(MAX_RETRY_DELAY_MS)
}

/// The clock the retry windows are measured on. Public so the retry loop and its tests read the
/// same source.
pub fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The on-disk codec. Hand-rolled rather than protobuf: the record is five fields, and a wire format
/// nobody else reads does not earn a schema. Lengths are bounded on encode so a pathological reason
/// string cannot make a row unreadable.
#[derive(Default)]
pub struct BlockDispositionCodec;

const TAG_PENDING: u8 = 0;
const TAG_FAULTED: u8 = 1;
const MAX_TEXT_BYTES: usize = 60_000;

fn bounded_text(text: &str) -> String {
    if text.len() <= MAX_TEXT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

impl Codec<BlockDisposition> for BlockDispositionCodec {
    fn encode(&self, value: &BlockDisposition) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        match value {
            BlockDisposition::Pending {
                block_number,
                stage,
                since_unix_ms,
            } => {
                out.push(TAG_PENDING);
                out.extend_from_slice(&i64::from(*block_number).to_le_bytes());
                out.extend_from_slice(&since_unix_ms.to_le_bytes());
                out.push(stage.as_byte());
            }
            BlockDisposition::Faulted {
                block_number,
                stage,
                reason,
                since_unix_ms,
                last_attempt_unix_ms,
                attempts,
            } => {
                out.push(TAG_FAULTED);
                out.extend_from_slice(&i64::from(*block_number).to_le_bytes());
                out.extend_from_slice(&since_unix_ms.to_le_bytes());
                out.extend_from_slice(&last_attempt_unix_ms.to_le_bytes());
                out.extend_from_slice(&attempts.to_le_bytes());
                let stage = bounded_text(stage);
                let reason = bounded_text(reason);
                let stage_len = u16::try_from(stage.len()).unwrap_or(u16::MAX);
                let reason_len = u16::try_from(reason.len()).unwrap_or(u16::MAX);
                out.extend_from_slice(&stage_len.to_le_bytes());
                out.extend_from_slice(stage.as_bytes());
                out.extend_from_slice(&reason_len.to_le_bytes());
                out.extend_from_slice(reason.as_bytes());
            }
        }
        out
    }

    fn decode(&self, bytes: &[u8]) -> Result<BlockDisposition, String> {
        let mut cursor = Cursor { bytes, at: 0 };
        let tag = cursor.byte()?;
        let block_number = BlockHeight::try_from(cursor.i64()?)
            .map_err(|e| format!("negative block height in disposition: {e}"))?;
        match tag {
            TAG_PENDING => {
                let since_unix_ms = cursor.u64()?;
                let stage = PendingStage::from_byte(cursor.byte()?)?;
                cursor.finish()?;
                Ok(BlockDisposition::Pending {
                    block_number,
                    stage,
                    since_unix_ms,
                })
            }
            TAG_FAULTED => {
                let since_unix_ms = cursor.u64()?;
                let last_attempt_unix_ms = cursor.u64()?;
                let attempts = cursor.u32()?;
                let stage = cursor.text()?;
                let reason = cursor.text()?;
                cursor.finish()?;
                Ok(BlockDisposition::Faulted {
                    block_number,
                    stage,
                    reason,
                    since_unix_ms,
                    last_attempt_unix_ms,
                    attempts,
                })
            }
            other => Err(format!("unknown block disposition tag {other}")),
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| "disposition record length overflow".to_string())?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("disposition record truncated at {}", self.at))?;
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(array))
    }

    fn i64(&mut self) -> Result<i64, String> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(i64::from_le_bytes(array))
    }

    fn text(&mut self) -> Result<String, String> {
        let len = usize::from(self.u16()?);
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("disposition text is not UTF-8: {e}"))
    }

    fn finish(&self) -> Result<(), String> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(format!(
                "disposition record has {} trailing bytes",
                self.bytes.len() - self.at
            ))
        }
    }
}

/// The per-shard ledger over an LMDB-backed typed store.
#[derive(Clone)]
pub struct BlockDispositionStore {
    store: Arc<dyn KeyValueTypedStore<BlockHash, BlockDisposition>>,
}

impl BlockDispositionStore {
    pub fn from_store(store: Arc<dyn KeyValueTypedStore<BlockHash, BlockDisposition>>) -> Self {
        Self { store }
    }

    /// **Record that this node owes the block an outcome.** Written when the body is in the store,
    /// before dependency resolution, so a crash or a receiver error cannot lose the obligation. An
    /// existing row is kept, so a re-delivery does not reset the fault's first sighting.
    pub async fn mark_pending(
        &self,
        hash: BlockHash,
        block_number: BlockHeight,
        stage: PendingStage,
    ) -> Result<(), String> {
        let since_unix_ms = match self.get(&hash).await? {
            // A fault is a stronger record than "pending"; a re-delivery must not erase it.
            Some(BlockDisposition::Faulted { .. }) => return Ok(()),
            Some(BlockDisposition::Pending { since_unix_ms, .. }) => since_unix_ms,
            None => now_unix_millis(),
        };
        self.store
            .put(&[(
                hash,
                BlockDisposition::Pending {
                    block_number,
                    stage,
                    since_unix_ms,
                },
            )])
            .await
    }

    /// Record a node fault and return the attempt count after the increment.
    ///
    /// A fault on a row that is already `Faulted` preserves the first sighting and increments; a
    /// fault on a `Pending` row starts the fault record at attempt 1.
    pub async fn mark_faulted(
        &self,
        hash: BlockHash,
        block_number: BlockHeight,
        stage: &str,
        reason: &str,
    ) -> Result<u32, String> {
        let now = now_unix_millis();
        let (since_unix_ms, attempts) = match self.get(&hash).await? {
            Some(BlockDisposition::Faulted {
                since_unix_ms,
                attempts,
                ..
            }) => (since_unix_ms, attempts.saturating_add(1)),
            Some(BlockDisposition::Pending { since_unix_ms, .. }) => (since_unix_ms, 1),
            None => (now, 1),
        };
        self.store
            .put(&[(
                hash,
                BlockDisposition::Faulted {
                    block_number,
                    stage: bounded_text(stage),
                    reason: bounded_text(reason),
                    since_unix_ms,
                    last_attempt_unix_ms: now,
                    attempts,
                },
            )])
            .await?;
        Ok(attempts)
    }

    /// A fault that cannot succeed on retry (self-equivocation is a proposer-only outcome on a
    /// received block) is recorded with the budget already spent, so no retry loop picks it up.
    /// It stays visible until an operator resets or repairs.
    pub async fn mark_terminal_fault(
        &self,
        hash: BlockHash,
        block_number: BlockHeight,
        stage: &str,
        reason: &str,
    ) -> Result<(), String> {
        let now = now_unix_millis();
        let since_unix_ms = match self.get(&hash).await? {
            Some(disposition) => match disposition {
                BlockDisposition::Faulted { since_unix_ms, .. } => since_unix_ms,
                BlockDisposition::Pending { since_unix_ms, .. } => since_unix_ms,
            },
            None => now,
        };
        self.store
            .put(&[(
                hash,
                BlockDisposition::Faulted {
                    block_number,
                    stage: bounded_text(stage),
                    reason: bounded_text(reason),
                    since_unix_ms,
                    last_attempt_unix_ms: now,
                    attempts: MAX_FAULT_RETRIES,
                },
            )])
            .await
    }

    /// Mark that a retry is now in flight, so the loop does not re-send it until the next window.
    pub async fn mark_retry_started(&self, hash: BlockHash) -> Result<(), String> {
        match self.get(&hash).await? {
            Some(BlockDisposition::Faulted {
                block_number,
                stage,
                reason,
                since_unix_ms,
                attempts,
                ..
            }) => {
                self.store
                    .put(&[(
                        hash,
                        BlockDisposition::Faulted {
                            block_number,
                            stage,
                            reason,
                            since_unix_ms,
                            last_attempt_unix_ms: now_unix_millis(),
                            attempts,
                        },
                    )])
                    .await
            }
            Some(BlockDisposition::Pending {
                block_number,
                since_unix_ms,
                ..
            }) => {
                self.store
                    .put(&[(
                        hash,
                        BlockDisposition::Pending {
                            block_number,
                            stage: PendingStage::Retry,
                            since_unix_ms,
                        },
                    )])
                    .await
            }
            None => Ok(()),
        }
    }

    /// **A verdict, or a fault the node has stopped retrying, settles the obligation.** The DAG
    /// metadata is the record for an accepted or refused block; a quarantined fault stays visible
    /// until an operator acts, so it is not deleted here.
    pub async fn settle(&self, hash: BlockHash) -> Result<usize, String> {
        self.store.delete(&[hash]).await
    }

    pub async fn get(&self, hash: &BlockHash) -> Result<Option<BlockDisposition>, String> {
        Ok(self.store.get(&[*hash]).await?.into_iter().next().flatten())
    }

    /// Every row the node still owes an outcome, oldest first by block height.
    pub async fn unsettled(&self) -> Result<Vec<(BlockHash, BlockDisposition)>, String> {
        let mut rows: Vec<(BlockHash, BlockDisposition)> =
            self.store.to_map().await?.into_iter().collect();
        rows.sort_by_key(|(_, disposition)| disposition.block_number());
        Ok(rows)
    }

    /// The rows whose retry window has opened and whose budget is not spent.
    pub async fn due_retries(
        &self,
        now_unix_ms: u64,
    ) -> Result<Vec<(BlockHash, BlockDisposition)>, String> {
        let mut due: Vec<(BlockHash, BlockDisposition)> = self
            .store
            .to_map()
            .await?
            .into_iter()
            .filter(|(_, disposition)| match disposition {
                BlockDisposition::Faulted {
                    last_attempt_unix_ms,
                    attempts,
                    ..
                } => {
                    *attempts < MAX_FAULT_RETRIES
                        && now_unix_ms.saturating_sub(*last_attempt_unix_ms)
                            >= retry_delay_ms(*attempts)
                }
                BlockDisposition::Pending { .. } => false,
            })
            .collect();
        due.sort_by_key(|(_, disposition)| disposition.block_number());
        Ok(due)
    }
}

/// How often the retry loop asks the ledger whether a fault's window has opened.
pub const RETRY_SWEEP_INTERVAL: Duration = Duration::from_secs(15);

/// **Re-offer every body this node still owes an outcome for.** Runs once at pipeline start, before
/// the node reports ready, so a crash between "body stored" and "DAG entry" is a retry rather than a
/// permanent orphan.
///
/// Rows are processed oldest first, and a row is skipped (not faulted) when a justification is not
/// yet in the DAG: the parent is also in the ledger and is sent earlier, so the processor's serial
/// order inserts it first; a genuinely missing parent leaves the row `Pending` for the next start
/// rather than spending its retry budget on a question that cannot be answered yet.
pub async fn reoffer_unsettled(
    ledger: BlockDispositionStore,
    block_store: BlockStore,
    dag: Arc<dyn BlockDagStorage>,
    to_processor: mpsc::Sender<BlockMessage>,
    log: Arc<dyn Log>,
) {
    let source = LogSource::new("casper.blocks.BlockDisposition");
    // **Do not re-offer into a DAG launch is still restoring.** A recovery replays into its own
    // staging DAG; a body offered to the live processor before the running engine exists would be
    // validated against a chain the node is not serving.
    crate::engine::node_phase::wait_until_running().await;
    let rows = match ledger.unsettled().await {
        Ok(rows) => rows,
        Err(e) => {
            log.error(
                source,
                &format!("could not read the disposition ledger at start-up: {e}"),
            );
            return;
        }
    };
    if rows.is_empty() {
        return;
    }
    let mut reoffered = 0usize;
    let mut skipped = 0usize;
    for (hash, disposition) in rows {
        if disposition.attempts() >= MAX_FAULT_RETRIES {
            log.error(
                source,
                &format!(
                    "block {} at height {} is quarantined after {} faults at stage {}; only an \
                     operator reset or an explicit repair clears it",
                    hash.to_hex(),
                    i64::from(disposition.block_number()),
                    disposition.attempts(),
                    disposition.stage_description()
                ),
            );
            continue;
        }
        let repr = dag.get_representation().await;
        if repr.contains(&hash) {
            // The crash landed after the insert, before the settle. The metadata is the verdict.
            if let Err(e) = ledger.settle(hash).await {
                log.warn(
                    source,
                    &format!(
                        "could not settle the already-inserted block {}: {e}",
                        hash.to_hex()
                    ),
                );
            }
            continue;
        }
        let stored = match block_store.get(&[hash]).await {
            Ok(stored) => stored,
            Err(e) => {
                log.error(
                    source,
                    &format!(
                        "could not read the stored body of block {}: {e}",
                        hash.to_hex()
                    ),
                );
                continue;
            }
        };
        let Some(block) = stored.into_iter().next().flatten() else {
            log.error(
                source,
                &format!(
                    "block {} has a {} disposition but no stored body; only an operator reset can \
                     settle it",
                    hash.to_hex(),
                    disposition.stage_description()
                ),
            );
            continue;
        };
        if !block
            .justifications
            .iter()
            .all(|parent| repr.contains(parent))
        {
            skipped = skipped.saturating_add(1);
            log.info(
                source,
                &format!(
                    "block {} still waits on a justification not in the DAG; keeping its {} \
                     disposition for a later attempt",
                    hash.to_hex(),
                    disposition.stage_description()
                ),
            );
            continue;
        }
        if let Err(e) = ledger.mark_retry_started(hash).await {
            log.error(
                source,
                &format!("could not mark block {} as re-offered: {e}", hash.to_hex()),
            );
        }
        if !chan::send(&to_processor, block).await {
            return;
        }
        reoffered = reoffered.saturating_add(1);
    }
    log.info(
        source,
        &format!(
            "start-up disposition audit: {reoffered} unsettled block(s) re-offered, {skipped} \
             skipped for missing parents"
        ),
    );
}

/// Re-offer a *stored* pending body after its parents have reached the DAG.
///
/// The boot sweep cannot replay a block whose parents are still missing, and the
/// fault loop intentionally only retries faulted rows. Without this sweep, a
/// previously skipped stored row can remain pending indefinitely after its
/// parent arrives. Only the Stored stage is eligible: Validating and Retry
/// indicate a block was already submitted to the processor. An age floor
/// excludes normal ingress handoffs and the boot-time sweep.
pub const PENDING_PARENT_RECHECK_AGE_MS: u64 = 60_000;

pub fn eligible_stored_pending(disposition: &BlockDisposition, now_ms: u64) -> bool {
    matches!(
        disposition,
        BlockDisposition::Pending {
            stage: PendingStage::Stored,
            since_unix_ms,
            ..
        } if now_ms.saturating_sub(*since_unix_ms) >= PENDING_PARENT_RECHECK_AGE_MS
    )
}

pub async fn retry_ready_stored_pending(
    ledger: BlockDispositionStore,
    block_store: BlockStore,
    dag: Arc<dyn BlockDagStorage>,
    to_processor: mpsc::Sender<BlockMessage>,
    log: Arc<dyn Log>,
) {
    let source = LogSource::new("casper.blocks.BlockDisposition");
    crate::engine::node_phase::wait_until_running().await;
    let mut interval = tokio::time::interval(RETRY_SWEEP_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Avoid racing the launch-time reoffer sweep on its first immediate tick.
    interval.tick().await;
    loop {
        interval.tick().await;
        let rows = match ledger.unsettled().await {
            Ok(rows) => rows,
            Err(e) => {
                log.error(source, &format!("pending parent sweep could not read ledger: {e}"));
                continue;
            }
        };
        let now_ms = now_unix_millis();
        for (hash, disposition) in rows {
            if !eligible_stored_pending(&disposition, now_ms) {
                continue;
            }
            let repr = dag.get_representation().await;
            if repr.contains(&hash) {
                if let Err(e) = ledger.settle(hash).await {
                    log.warn(source, &format!("could not settle inserted pending block {}: {e}", hash.to_hex()));
                }
                continue;
            }
            let stored = match block_store.get(&[hash]).await {
                Ok(stored) => stored,
                Err(e) => {
                    log.error(source, &format!("could not load stored pending body {}: {e}", hash.to_hex()));
                    continue;
                }
            };
            let Some(block) = stored.into_iter().next().flatten() else {
                log.error(source, &format!("pending body {} is missing from storage; retaining obligation", hash.to_hex()));
                continue;
            };
            if !block.justifications.iter().all(|parent| repr.contains(parent)) {
                continue;
            }
            // The stage transition prevents further sweeps from dispatching the
            // same body while its validation is in flight. Never erase faults.
            if let Err(e) = ledger.mark_retry_started(hash).await {
                log.error(source, &format!("could not reserve pending replay {}: {e}", hash.to_hex()));
                continue;
            }
            log.info(source, &format!("re-offering stored pending body {} after parents entered DAG", hash.to_hex()));
            if !chan::send(&to_processor, block).await {
                return;
            }
        }
    }
}

/// The retry loop: every [`RETRY_SWEEP_INTERVAL`], re-offer the faulted blocks whose backoff has
/// elapsed. The processor's fault handlers update the row, so a repeated fault counts and a
/// successful one settles it. A terminal fault stays in the ledger and keeps the pipeline fault
/// surface loud; nothing here clears it without an operator.
pub async fn retry_faulted_blocks(
    ledger: BlockDispositionStore,
    block_store: BlockStore,
    to_processor: mpsc::Sender<BlockMessage>,
    log: Arc<dyn Log>,
) {
    let source = LogSource::new("casper.blocks.BlockDisposition");
    crate::engine::node_phase::wait_until_running().await;
    let mut interval = tokio::time::interval(RETRY_SWEEP_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let due = match ledger.due_retries(now_unix_millis()).await {
            Ok(due) => due,
            Err(e) => {
                log.error(
                    source,
                    &format!("could not read the disposition ledger; retry sweep skipped: {e}"),
                );
                continue;
            }
        };
        for (hash, disposition) in due {
            let stored = match block_store.get(&[hash]).await {
                Ok(stored) => stored,
                Err(e) => {
                    log.error(
                        source,
                        &format!(
                            "could not read the stored body of block {}: {e}",
                            hash.to_hex()
                        ),
                    );
                    continue;
                }
            };
            let Some(block) = stored.into_iter().next().flatten() else {
                log.error(
                    source,
                    &format!(
                        "faulted block {} has no stored body; the fault stays and only an \
                         operator reset can settle it",
                        hash.to_hex()
                    ),
                );
                continue;
            };
            // **Update the window before the send**, so a send that faults and returns to this loop
            // does not re-offer the same block on the next tick.
            if let Err(e) = ledger.mark_retry_started(hash).await {
                log.error(
                    source,
                    &format!("could not mark block {} as retried: {e}", hash.to_hex()),
                );
            }
            log.warn(
                source,
                &format!(
                    "re-offering faulted block #{} {} (attempt {} of {}, stage {})",
                    block.block_number,
                    hash.to_hex(),
                    disposition.attempts().saturating_add(1),
                    MAX_FAULT_RETRIES,
                    disposition.stage_description()
                ),
            );
            if !chan::send(&to_processor, block).await {
                return;
            }
        }
    }
}

/// How often the stored-body audit samples the two counts.
pub const ORPHAN_AUDIT_INTERVAL: Duration = Duration::from_secs(60);

/// **How many consecutive samples a store/DAG gap must persist before it is an orphan.** Every
/// healthy node has a body stored for a moment before its DAG entry exists; treating one sample as
/// an orphan would flap the readiness gate on every block. Three minutes at the default interval is
/// long past any legitimate in-flight block and short enough that the live incident's first hour
/// would have been loud.
pub const ORPHAN_AUDIT_SAMPLES: u32 = 3;

/// The confirmed gap — bodies in the block store that the DAG does not account for — and how many
/// times a sustained gap has been entered. Process-wide (the gate and `/health` are per node); a
/// shard-local audit writes the sum, which is the conservative direction for readiness.
static ORPHAN_BODIES: AtomicU64 = AtomicU64::new(0);
static ORPHAN_DETECTIONS: AtomicU64 = AtomicU64::new(0);

// A boot-time store/DAG mismatch is known before readiness is exposed. The
// periodic three-sample rule only protects against transient *runtime* writes;
// it must not erase an already observed startup discrepancy.
pub fn observe_boot_orphan_gap(stored: usize, in_dag: usize) -> u64 {
    let gap = u64::try_from(stored.saturating_sub(in_dag)).unwrap_or(u64::MAX);
    if gap > 0 {
        let previous = ORPHAN_BODIES.swap(gap, Ordering::Relaxed);
        if previous == 0 {
            ORPHAN_DETECTIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
    gap
}

/// The currently observed orphan count. A mismatch already known at boot blocks readiness
/// immediately; a transient runtime write counts only after three consecutive audit samples.
pub fn orphan_bodies() -> u64 {
    ORPHAN_BODIES.load(Ordering::Relaxed)
}

/// Clear the readiness blocker only when the store/DAG discrepancy is actually gone.
fn clear_orphan_gate_if_resolved(gap: u64) -> bool {
    gap == 0 && ORPHAN_BODIES.swap(0, Ordering::Relaxed) > 0
}

/// How many distinct sustained orphan episodes this process has seen. Monotone, so an operator can
/// tell "the gate is clear because the gap closed" from "the gate never tripped".
pub fn orphan_detections() -> u64 {
    ORPHAN_DETECTIONS.load(Ordering::Relaxed)
}

/// The consecutive-sample rule, separated so it can be tested without a node: returns `Some(gap)`
/// exactly when a gap has been observed `ORPHAN_AUDIT_SAMPLES` times running.
#[derive(Default)]
pub struct OrphanStreak {
    samples: u32,
}

impl OrphanStreak {
    pub fn observe(&mut self, gap: u64) -> Option<u64> {
        if gap == 0 {
            self.samples = 0;
            return None;
        }
        self.samples = self.samples.saturating_add(1);
        if self.samples >= ORPHAN_AUDIT_SAMPLES {
            Some(gap)
        } else {
            None
        }
    }
}

/// **A body the DAG does not account for is a fault even when no ledger row names it** (U19,
/// F-U19-07). This is the periodic audit the start-up re-offer cannot be: it compares the two
/// counts a healthy shard keeps equal — one record in the block store per block in the DAG index —
/// and reports a *sustained* difference. A body written by a pre-ledger build lands here; so does
/// any future accounting bug. The audit does not delete or repair: it makes the state loud and
/// withholds readiness, and the operator reset remains the repair.
pub async fn audit_stored_bodies(
    block_store: BlockStore,
    dag: Arc<dyn BlockDagStorage>,
    log: Arc<dyn Log>,
) {
    let source = LogSource::new("casper.blocks.BlockDisposition");
    crate::engine::node_phase::wait_until_running().await;
    let mut interval = tokio::time::interval(ORPHAN_AUDIT_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut streak = OrphanStreak::default();
    loop {
        interval.tick().await;
        let stored = match block_store.count().await {
            Ok(count) => u64::try_from(count).unwrap_or(u64::MAX),
            Err(e) => {
                log.error(
                    source,
                    &format!("stored-body audit could not count the block store: {e}"),
                );
                continue;
            }
        };
        let in_dag =
            u64::try_from(dag.get_representation().await.dag_set.len()).unwrap_or(u64::MAX);
        let gap = stored.saturating_sub(in_dag);
        match streak.observe(gap) {
            Some(gap) => {
                let previous = ORPHAN_BODIES.swap(gap, Ordering::Relaxed);
                if previous == 0 {
                    ORPHAN_DETECTIONS.fetch_add(1, Ordering::Relaxed);
                    log.error(
                        source,
                        &format!(
                            "{gap} stored block body/bodies have no DAG entry for \
                             {ORPHAN_AUDIT_SAMPLES} consecutive samples ({stored} stored, {in_dag} \
                             in the DAG); the readiness gate is now blocked. The start-up re-offer \
                             covers bodies with a disposition row; a body without one is a \
                             pre-ledger orphan and needs an operator reset or a peer re-delivery"
                        ),
                    );
                }
            }
            None => {
                // An already confirmed boot gap remains a readiness blocker
                // until an observation proves the discrepancy has vanished.
                if clear_orphan_gate_if_resolved(gap) {
                    log.info(
                        source,
                        &format!(
                            "the stored-body/DAG gap closed ({stored} stored, {in_dag} in the DAG)"
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_block_storage::dag::codecs::BlockHashCodec;
    use rchain_shared::store_manager::{database, InMemoryStoreManager};

    fn hash(byte: u8) -> BlockHash {
        BlockHash::new([byte; 32])
    }

    fn height(n: i64) -> BlockHeight {
        BlockHeight::try_from(n).unwrap()
    }

    async fn store() -> BlockDispositionStore {
        let manager = InMemoryStoreManager::default();
        let kv = database(
            &manager,
            "block-disposition",
            Arc::new(BlockHashCodec),
            Arc::new(BlockDispositionCodec),
        )
        .await
        .unwrap();
        BlockDispositionStore::from_store(Arc::new(kv))
    }

    #[test]
    fn the_disposition_codec_round_trips_both_states() {
        let codec = BlockDispositionCodec;
        for disposition in [
            BlockDisposition::Pending {
                block_number: height(7),
                stage: PendingStage::Stored,
                since_unix_ms: 1_700_000_000_000,
            },
            BlockDisposition::Faulted {
                block_number: height(9),
                stage: "validation internal".to_string(),
                reason: "merge search budget refused".to_string(),
                since_unix_ms: 1_700_000_000_001,
                last_attempt_unix_ms: 1_700_000_000_002,
                attempts: 3,
            },
        ] {
            let bytes = codec.encode(&disposition);
            assert_eq!(codec.decode(&bytes).unwrap(), disposition);
        }
    }

    #[test]
    fn a_transient_gap_is_not_an_orphan_but_a_sustained_one_is() {
        let mut streak = OrphanStreak::default();
        assert_eq!(streak.observe(1), None, "one in-flight body is normal");
        assert_eq!(streak.observe(1), None);
        assert_eq!(
            streak.observe(1),
            Some(1),
            "the third consecutive sample is an orphan"
        );
        // A zero sample is evidence the normal case returned; the count starts again.
        assert_eq!(streak.observe(0), None);
        assert_eq!(streak.observe(1), None);
    }

    #[test]
    fn a_known_boot_gap_is_not_hidden_behind_the_periodic_sampling_window() {
        ORPHAN_BODIES.store(0, Ordering::Relaxed);
        // This exact shape occurred in the isolated legacy testnet snapshot.
        assert_eq!(observe_boot_orphan_gap(1947, 21), 1926);
        assert_eq!(orphan_bodies(), 1926);
        // One transient sample cannot clear a mismatch already found at boot.
        let mut streak = OrphanStreak::default();
        assert_eq!(streak.observe(1926), None);
        assert!(!clear_orphan_gate_if_resolved(1926));
        assert_eq!(orphan_bodies(), 1926);
        assert!(clear_orphan_gate_if_resolved(0));
        assert_eq!(orphan_bodies(), 0);
        assert_eq!(observe_boot_orphan_gap(21, 1947), 0);
        assert_eq!(orphan_bodies(), 0);
    }

    #[test]
    fn a_truncated_or_trailing_record_is_an_error_not_a_default() {
        let codec = BlockDispositionCodec;
        let bytes = codec.encode(&BlockDisposition::Pending {
            block_number: height(1),
            stage: PendingStage::Stored,
            since_unix_ms: 5,
        });
        assert!(codec.decode(&bytes[..bytes.len() - 1]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(codec.decode(&longer).is_err());
    }

    #[tokio::test]
    async fn a_pending_row_survives_redelivery_and_settles_only_on_a_verdict() {
        let store = store().await;
        let h = hash(1);
        store
            .mark_pending(h, height(3), PendingStage::Stored)
            .await
            .unwrap();
        let since = match store.get(&h).await.unwrap().unwrap() {
            BlockDisposition::Pending { since_unix_ms, .. } => since_unix_ms,
            other => panic!("expected a pending row, got {other:?}"),
        };
        store
            .mark_pending(h, height(3), PendingStage::Validating)
            .await
            .unwrap();
        match store.get(&h).await.unwrap().unwrap() {
            BlockDisposition::Pending {
                stage,
                since_unix_ms,
                ..
            } => {
                assert_eq!(stage, PendingStage::Validating);
                assert_eq!(
                    since_unix_ms, since,
                    "a re-delivery does not restart the clock"
                );
            }
            other => panic!("expected a pending row, got {other:?}"),
        }
        store.settle(h).await.unwrap();
        assert_eq!(store.get(&h).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_redelivered_faulted_block_is_not_downgraded_to_pending() {
        let store = store().await;
        let h = hash(4);
        store
            .mark_faulted(h, height(6), "internal", "boom")
            .await
            .unwrap();
        store
            .mark_pending(h, height(6), PendingStage::Stored)
            .await
            .unwrap();
        assert!(matches!(
            store.get(&h).await.unwrap(),
            Some(BlockDisposition::Faulted { attempts: 1, .. })
        ));
    }

    #[tokio::test]
    async fn faults_increment_and_preserve_the_first_sighting() {
        let store = store().await;
        let h = hash(2);
        store
            .mark_pending(h, height(4), PendingStage::Stored)
            .await
            .unwrap();
        assert_eq!(
            store
                .mark_faulted(h, height(4), "internal", "boom")
                .await
                .unwrap(),
            1
        );
        let attempts = store
            .mark_faulted(h, height(4), "internal", "boom again")
            .await
            .unwrap();
        assert_eq!(attempts, 2);
        match store.get(&h).await.unwrap().unwrap() {
            BlockDisposition::Faulted {
                attempts, reason, ..
            } => {
                assert_eq!(attempts, 2);
                assert_eq!(reason, "boom again");
            }
            other => panic!("expected a fault, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_retry_is_not_due_before_its_backoff_and_not_after_the_budget() {
        let store = store().await;
        let h = hash(3);
        store
            .mark_faulted(h, height(5), "insert", "store error")
            .await
            .unwrap();
        // The first attempt's last_attempt is now; nothing is due immediately.
        let now = now_unix_millis();
        assert!(store
            .due_retries(now.saturating_sub(FIRST_RETRY_DELAY_MS / 2))
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .due_retries(now.saturating_add(MAX_RETRY_DELAY_MS))
                .await
                .unwrap()
                .len(),
            1
        );
        for _ in 0..MAX_FAULT_RETRIES {
            store
                .mark_faulted(h, height(5), "insert", "store error")
                .await
                .unwrap();
        }
        assert!(store
            .due_retries(now.saturating_add(MAX_RETRY_DELAY_MS))
            .await
            .unwrap()
            .is_empty());
    }
}
