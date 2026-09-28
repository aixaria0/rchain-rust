//! Block validation status (port of `BlockStatus.scala`).

/// The outcome of validating a block (port of `BlockStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockStatus {
    Valid,
    InvalidBlockNumber,
    InvalidRepeatDeploy,
    InvalidSequenceNumber,
    InvalidDeployShardId,
    JustificationRegression,
    NeglectedInvalidBlock,
    InvalidStateHash,
    InvalidBondsCache,
    InvalidRejectedDeploy,
    ContainsExpiredDeploy,
    ContainsFutureDeploy,
    ContainsLowCostDeploy,
    /// A deploy's phlo limit is negative (AUDIT C109). Its own status rather than folding into
    /// `ContainsLowCostDeploy`: a negative *limit* is not a cheap deploy, it is a malformed one whose
    /// charge would be a credit, and a validator that reports the two as the same thing cannot say
    /// which it rejected.
    InvalidPhloLimit,
    /// The block slashes a validator that none of its justifications holds responsible (AUDIT C110).
    ///
    /// A `Slash` is a *system* deploy: it is not signed by its victim, it moves that victim's entire
    /// bond to the Coop vault, and every validator re-executes it during replay. Until this status
    /// existed, a block was accepted on the strength of its post-state hash alone — a proposer could
    /// name any bonded validator and, provided it computed the resulting state honestly, every other
    /// validator would help it confiscate the stake. The rule is now re-derived on the receiving side
    /// from the node's own block metadata, not taken from the proposer.
    UnjustifiedSlash,
    /// A deploy in the block is not signed by the key its `deployer` field names (AUDIT C120).
    ///
    /// **Why this is a separate status and not a reuse of `InvalidRepeatDeploy`.** The replay reads the
    /// deployer out of this field to build the pre-charge, the refund and the `rho:rchain:deployerId`
    /// binding, so an unverified field is an *authorization* claim rather than a malformed one: a
    /// proposer could name any account, put arbitrary bytes in `sig`, and have every other validator
    /// debit that account and pay the proposer's term — with a post-state hash they computed honestly,
    /// so the block was valid and nothing was attributable. `verify_signature` existed and was correct;
    /// it was called only at the deploy *ingress*, never on the path a peer's block takes. A validator
    /// that reports this must be able to say that is what it rejected.
    ///
    /// `system_deploys` are exempt by construction and are not inspected: a `Slash` is unsigned by its
    /// victim (see `UnjustifiedSlash` above), and the rule that makes one legitimate is a different
    /// check entirely.
    InvalidDeploySignature,
    /// A block carries more deploys than the protocol's seed index can address (AUDIT F-3).
    ///
    /// The proposer has always bounded its *own* selection, because `close_block` indexes the deploy
    /// randomness seed in a `u8` and the deploy count plus the slash count must fit in 255. Nothing
    /// bounded what a validator would *accept*: a peer's block could carry any number of deploys and
    /// every node would replay all of them. The bound is a length, so it costs nothing to apply, and it
    /// belongs among the pre-replay checks for the same reason `phlo_price` does.
    TooManyDeploys,
    /// A block's total declared phlo exceeds the block budget (AUDIT F-3).
    ///
    /// Without this there is no bound on what one block costs: the per-deploy budget bounds each deploy
    /// separately, so a proposer could pack the block arbitrarily full and every validator would replay
    /// the lot. Sui and Solana both contain a block's blast radius this way; this is the port's
    /// equivalent, and it is a deliberate divergence — the Scala `blockSummary` composes no per-block
    /// bound at all.
    ExceedsBlockPhloLimit,
}

impl BlockStatus {
    pub fn is_valid(&self) -> bool {
        matches!(self, BlockStatus::Valid)
    }
}

impl std::fmt::Display for BlockStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BlockStatus::Valid => "valid",
            BlockStatus::InvalidBlockNumber => "invalid block number",
            BlockStatus::InvalidRepeatDeploy => "a deploy was repeated across blocks",
            BlockStatus::InvalidSequenceNumber => "invalid sender sequence number",
            BlockStatus::InvalidDeployShardId => "deploy shard id does not match the block's shard",
            BlockStatus::JustificationRegression => "a justification regressed from its parent",
            BlockStatus::NeglectedInvalidBlock => "an invalid block was used as a justification",
            BlockStatus::InvalidStateHash => {
                "the block's declared post-state hash does not match the state recomputed by \
                 replaying its deploys — a node state-accounting inconsistency, not an error in your \
                 deploy or API call"
            }
            BlockStatus::InvalidBondsCache => "invalid bonds cache",
            BlockStatus::InvalidRejectedDeploy => "the block's rejected-deploy set does not match its parents",
            BlockStatus::ContainsExpiredDeploy => "a deploy has expired",
            BlockStatus::ContainsFutureDeploy => "a deploy has a future validity window",
            BlockStatus::ContainsLowCostDeploy => "a deploy's phlo price is below the minimum",
            BlockStatus::InvalidPhloLimit => "a deploy has a negative phlo limit",
            BlockStatus::UnjustifiedSlash => "the block slashes a validator none of its justifications holds responsible",
            BlockStatus::InvalidDeploySignature => {
                "a deploy is not signed by the key its `deployer` field names, so the account this \
                 block charges — and pays — was chosen by the block's author rather than proven by a \
                 signature"
            }
            BlockStatus::TooManyDeploys => {
                "the block carries more deploys than the protocol's seed index can address"
            }
            BlockStatus::ExceedsBlockPhloLimit => {
                "the block's total declared phlo exceeds the per-block budget"
            }
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every status, so a variant added without a message (or with a copy-pasted one) fails here.
    const ALL: [BlockStatus; 18] = [
        BlockStatus::Valid,
        BlockStatus::InvalidBlockNumber,
        BlockStatus::InvalidRepeatDeploy,
        BlockStatus::InvalidSequenceNumber,
        BlockStatus::InvalidDeployShardId,
        BlockStatus::JustificationRegression,
        BlockStatus::NeglectedInvalidBlock,
        BlockStatus::InvalidStateHash,
        BlockStatus::InvalidBondsCache,
        BlockStatus::InvalidRejectedDeploy,
        BlockStatus::ContainsExpiredDeploy,
        BlockStatus::ContainsFutureDeploy,
        BlockStatus::ContainsLowCostDeploy,
        BlockStatus::InvalidPhloLimit,
        BlockStatus::UnjustifiedSlash,
        BlockStatus::InvalidDeploySignature,
        BlockStatus::TooManyDeploys,
        BlockStatus::ExceedsBlockPhloLimit,
    ];

    /// `Valid` is the **only** status that is valid: `is_valid` is the one predicate the block
    /// processor branches on, and a non-`Valid` status that answered `true` would admit an invalid
    /// block into the DAG.
    #[test]
    fn only_the_valid_status_is_valid() {
        assert!(BlockStatus::Valid.is_valid());
        for status in ALL {
            if status != BlockStatus::Valid {
                assert!(!status.is_valid(), "{status:?} must not be valid");
            }
        }
    }

    /// Every status renders a distinct, non-empty sentence — these reach an operator through the
    /// API when a block is rejected, and two statuses sharing a message would make the rejection
    /// undiagnosable. The `InvalidStateHash` message is the longest and explains that the mismatch
    /// is an interpreter/state-accounting fault rather than a malformed deploy, so it is asserted
    /// in full.
    #[test]
    fn every_status_renders_a_distinct_message() {
        let mut messages: Vec<String> = Vec::new();
        for status in ALL {
            let message = status.to_string();
            assert!(!message.is_empty(), "{status:?} has no message");
            assert!(
                !messages.contains(&message),
                "two statuses share a message: {message}"
            );
            messages.push(message);
        }
        assert_eq!(messages.len(), ALL.len());

        assert_eq!(BlockStatus::Valid.to_string(), "valid");
        assert_eq!(
            BlockStatus::InvalidStateHash.to_string(),
            "the block's declared post-state hash does not match the state recomputed by replaying \
             its deploys — a node state-accounting inconsistency, not an error in your deploy or API \
             call"
        );
        assert_eq!(
            BlockStatus::InvalidDeployShardId.to_string(),
            "deploy shard id does not match the block's shard"
        );
        assert_eq!(
            BlockStatus::ContainsLowCostDeploy.to_string(),
            "a deploy's phlo price is below the minimum"
        );
    }

    /// The status is a small `Copy` value that hashes: the block processor keeps it in maps and
    /// compares it, so `Eq`/`Hash` must agree (`Valid` equal to itself, distinct from the rest).
    #[test]
    fn the_status_compares_and_hashes_by_value() {
        use std::collections::HashSet;

        let copied = BlockStatus::Valid;
        assert_eq!(copied, BlockStatus::Valid);

        let set: HashSet<BlockStatus> = ALL.into_iter().collect();
        assert_eq!(set.len(), ALL.len(), "all of them are distinct");

        // A rejected block's status is never equal to `Valid`, which is what the caller tests.
        assert_ne!(BlockStatus::InvalidBondsCache, BlockStatus::Valid);
    }
}
