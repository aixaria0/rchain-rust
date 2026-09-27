//! The PoS read surface the node did not have (AUDIT C148).
//!
//! `docs/src/node/operating.md` explains the epoch rule ("withdraw … only stages a deadline
//! (`pendingWithdrawers`) … the boundary is `blockNumber % epochLength == 0`") and then leaves the
//! operator to derive the boundary from block heights: the epoch counter, the active validator set
//! and `pendingWithdrawers` live in native state and were projected by no route and no subcommand.
//! `bond-status` answers a bool; `GET /api/v1/shards` answers heights. So an epoch or unbonding
//! question got answered by inference, or by hand-deploying rholang from a console on a live
//! validator — exactly the poking the REPL's isolated eval store exists to avoid for terms.
//!
//! **A trait of its own rather than a method on `BlockApi`.** The read needs the shard's
//! `RuntimeManager` and the status API, neither of which belongs on the block API — and adding a
//! method to `BlockApi` would have meant touching every mock of it in the tree, which is a large
//! diff for a read that no block-path caller wants.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rchain_casper::runtime_manager::RuntimeManager;
use rchain_models::validator::Validator;
use rchain_rholang::native_state::NativeSystemState;

use crate::api::web_api::WebApi;

/// A withdrawal staged and waiting for its boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingWithdrawal {
    pub validator: Validator,
    /// The block number the withdrawal was staged at (`pendingWithdrawers`' value).
    pub staged_at_block: i64,
    /// Blocks still to be produced before the boundary pays it out — the number the row's own
    /// trigger says an operator currently has to derive by hand. Never negative; a withdrawal whose
    /// quarantine has already elapsed reads `0` rather than wrapping.
    pub blocks_remaining: i64,
}

/// What the node can say about its own PoS state, in one read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PosStatus {
    pub latest_block_number: i64,
    /// `epochLength` from the shard's PoS parameters (`<= 1` means every block is a boundary).
    pub epoch_length: i64,
    /// `quarantineLength` — the distance between a staged withdrawal and its payout.
    pub quarantine_length: i64,
    /// The current epoch: the number of boundaries behind the head. `0` while `epochLength <= 1`,
    /// because then every block *is* a boundary and counting them would name nothing.
    pub epoch: i64,
    /// Blocks to the next boundary: `0` exactly when the head is one.
    pub blocks_until_epoch_boundary: i64,
    pub active_validators: Vec<Validator>,
    pub pending_withdrawals: Vec<PendingWithdrawal>,
}

/// The PoS read, as the HTTP layer sees it.
#[async_trait]
pub trait PosReadApi: Send + Sync {
    async fn pos_status(&self) -> Result<PosStatus, String>;
}

/// The real read: the primary shard's live native state, plus the status API for the head's height.
pub struct ShardPosRead {
    runtime: Arc<RuntimeManager>,
    web_api: Arc<dyn WebApi>,
}

impl ShardPosRead {
    pub fn new(runtime: Arc<RuntimeManager>, web_api: Arc<dyn WebApi>) -> Self {
        ShardPosRead { runtime, web_api }
    }
}

#[async_trait]
impl PosReadApi for ShardPosRead {
    async fn pos_status(&self) -> Result<PosStatus, String> {
        let native = NativeSystemState::new(self.runtime.runtime().native_store());
        let params = native.params().await?;
        let active = native.active_validators().await?;
        let pending = native.pending_withdrawers().await?;
        // The head's height, from the same read `/api/status` answers with — one definition of
        // "latest block" in the node rather than a second one here.
        let latest = self
            .web_api
            .status()
            .await
            .map_err(|e| e.to_string())?
            .latest_block_number;

        Ok(PosStatus {
            latest_block_number: latest,
            epoch_length: params.epoch_length,
            quarantine_length: params.quarantine_length,
            epoch: epoch_of(latest, params.epoch_length),
            blocks_until_epoch_boundary: blocks_until_boundary(latest, params.epoch_length),
            active_validators: active.into_iter().collect(),
            pending_withdrawals: pending
                .into_iter()
                .map(|(validator, staged_at_block)| PendingWithdrawal {
                    validator,
                    staged_at_block,
                    blocks_remaining: blocks_remaining(
                        latest,
                        staged_at_block,
                        params.quarantine_length,
                    ),
                })
                .collect(),
        })
    }
}

/// How many boundaries have passed at `block_number`.
///
/// `epochLength <= 1` is the "every block is a boundary" setting (`PosParams`'s own words), where
/// counting boundaries would name a number no rule reads — so it answers `0` rather than dividing.
fn epoch_of(block_number: i64, epoch_length: i64) -> i64 {
    if epoch_length <= 1 {
        0
    } else {
        block_number.div_euclid(epoch_length)
    }
}

/// Blocks until the next boundary; `0` at a boundary, and `0` when every block is one.
fn blocks_until_boundary(block_number: i64, epoch_length: i64) -> i64 {
    if epoch_length <= 1 {
        0
    } else {
        epoch_length - block_number.rem_euclid(epoch_length)
    }
}

/// Blocks left before a withdrawal staged at `staged_at_block` is paid out, floored at zero.
fn blocks_remaining(latest: i64, staged_at_block: i64, quarantine_length: i64) -> i64 {
    let due = staged_at_block.saturating_add(quarantine_length);
    due.saturating_sub(latest).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic the operator is currently expected to do by hand. Both directions and the
    /// degenerate setting, because `epochLength` of `1` (or `0`) is the shape a modulo by it would
    /// divide by — and "every block is a boundary" is a real genesis setting, not a corner case.
    #[test]
    fn the_boundary_arithmetic_answers_the_operators_question() {
        // The default devnet epoch length of the genesis PoS parameters.
        assert_eq!(epoch_of(250, 100), 2);
        assert_eq!(blocks_until_boundary(250, 100), 50);
        // On a boundary: the next one is a whole epoch away, not zero away.
        assert_eq!(epoch_of(300, 100), 3);
        assert_eq!(blocks_until_boundary(300, 100), 100);
        assert_eq!(blocks_until_boundary(0, 100), 100);

        // `epochLength <= 1`: every block is a boundary, so nothing is counted and nothing is
        // waited for — and nothing divides by zero.
        assert_eq!(epoch_of(42, 1), 0);
        assert_eq!(blocks_until_boundary(42, 1), 0);
        assert_eq!(epoch_of(42, 0), 0);
        assert_eq!(blocks_until_boundary(42, 0), 0);
    }

    /// A staged withdrawal counts down to its payout and stops at zero — the number the docs tell
    /// the operator to derive from the epoch boundary, which is exactly what this replaces.
    #[test]
    fn a_pending_withdrawal_counts_down_and_never_goes_negative() {
        // Staged at 250 with a quarantine of 100 → due at 350.
        assert_eq!(blocks_remaining(250, 250, 100), 100);
        assert_eq!(blocks_remaining(300, 250, 100), 50);
        assert_eq!(blocks_remaining(350, 250, 100), 0);
        // Past due is zero rather than a negative countdown: a withdrawal that has not been paid
        // out yet is not an operator error to be reported as a negative number of blocks.
        assert_eq!(blocks_remaining(400, 250, 100), 0);
        // The degenerate quarantine setting.
        assert_eq!(blocks_remaining(10, 10, 0), 0);
    }
}
