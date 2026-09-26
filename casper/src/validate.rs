//! Block validation predicates (port of `Validate.scala`) — the pure, effect-free checks.

use rchain_models::block_hash::BlockHash;
use rchain_models::block_version::SUPPORTED;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_shared::refined::ShardId;

use crate::block_status::BlockStatus;
use crate::proto_util::hash_block;

/// Validate that the block's identifying fields are non-empty (port of `formatOfFields`).
pub fn format_of_fields(b: &BlockMessage) -> bool {
    if b.block_hash == BlockHash::new([0u8; 32]) {
        false
    } else if b.sig.is_empty() {
        false
    } else if b.sig_algorithm.is_empty() {
        false
    } else if ShardId::try_from(b.shard_id.clone()).is_err() {
        // A non-empty, ASCII shard id (Law 26) — the validated constructor is the single ingress
        // check, rather than the empty + non-ASCII pair it replaces (and the `debug_assert!` inside
        // `BlockRandomSeed::new`).
        false
    } else {
        true
    }
}

/// Validate that the block version is supported (port of `version`).
pub fn version(b: &BlockMessage) -> bool {
    SUPPORTED.contains(&b.version)
}

/// Validate that the block hash matches its content-addressed value (Law 16; port of `blockHash`).
pub fn block_hash(b: &BlockMessage) -> bool {
    b.block_hash == hash_block(b)
}

/// Validate the block signature against the sender's public key (port of `blockSignature`).
pub fn block_signature(b: &BlockMessage) -> bool {
    match rchain_crypto::signatures::signatures_alg::from_algorithm(&b.sig_algorithm) {
        Some(alg) => alg.verify(b.block_hash.as_bytes(), &b.sig, b.sender.as_bytes()),
        None => false,
    }
}

/// Validate that no deploy is scheduled for a future block (port of `futureTransaction`).
pub fn future_transaction(b: &BlockMessage) -> BlockStatus {
    if b.state
        .deploys
        .iter()
        .any(|d| d.deploy.data.valid_after_block_number > i64::from(b.block_number))
    {
        BlockStatus::ContainsFutureDeploy
    } else {
        BlockStatus::Valid
    }
}

/// Validate that no deploy has expired (port of `transactionExpiration`).
pub fn transaction_expiration(b: &BlockMessage, expiration_threshold: i64) -> BlockStatus {
    let earliest = b.block_number - expiration_threshold;
    if b.state
        .deploys
        .iter()
        .any(|d| d.deploy.data.valid_after_block_number <= earliest)
    {
        BlockStatus::ContainsExpiredDeploy
    } else {
        BlockStatus::Valid
    }
}

/// Validate that all deploys belong to the validator's shard (port of `deploysShardIdentifier`).
pub fn deploys_shard_identifier(b: &BlockMessage, shard_id: &str) -> BlockStatus {
    if b.state
        .deploys
        .iter()
        .all(|d| d.deploy.data.shard_id == shard_id)
    {
        BlockStatus::Valid
    } else {
        BlockStatus::InvalidDeployShardId
    }
}

/// Validate that all deploys meet the minimum phlo price (port of `phloPrice`).
pub fn phlo_price(b: &BlockMessage, min_phlo_price: i64) -> BlockStatus {
    if b.state
        .deploys
        .iter()
        .all(|d| d.deploy.data.phlo_price >= min_phlo_price)
    {
        BlockStatus::Valid
    } else {
        BlockStatus::ContainsLowCostDeploy
    }
}

/// Validate that no deploy carries a negative phlo limit (AUDIT C109).
///
/// **Why this check has to exist on this path.** The charge is `phlo_limit × phlo_price`, and it is a
/// debit: it reaches `native.pre_charge`, which subtracts it from the deployer's vault. A negative
/// limit made that subtraction an addition — `balance - (-n) == balance + n` — crediting the deployer
/// out of nothing while the staking-vault side of the transfer silently no-opped. The deploy-ingress
/// path checked for it (`BlockApiImpl::deploy`), so a *local* deploy could not carry one; but this
/// function's callers are the checks a validator runs on **another node's block**, where the deploy
/// data comes off the wire and nothing had looked at the sign. That asymmetry is exactly the shape
/// that let the bug be reachable only by a peer.
///
/// `total_phlo_charge` now refuses a negative product at the type level, so the replay path cannot
/// mint even without this check. This one is here because a validator should *reject the block*, not
/// fail somewhere downstream: a `BlockStatus` says which rule was broken, and "the charge could not
/// be computed" during replay does not.
pub fn phlo_limit(b: &BlockMessage) -> BlockStatus {
    if b.state.deploys.iter().all(|d| d.deploy.data.phlo_limit >= 0) {
        BlockStatus::Valid
    } else {
        BlockStatus::InvalidPhloLimit
    }
}

/// The validators a block's own justifications hold responsible for an attributable failure
/// (AUDIT C110). This is the *rule*; the proposer narrows it further (see
/// [`crate::blocks::proposer::proposer`], which additionally requires the offender to be bonded — a
/// proposer's choice about who is worth slashing, not part of what makes a slash justified).
///
/// **Why this is one function and not two.** The proposer decided who to slash from this rule, and
/// the receiving validators used to take that decision on trust: they replayed the `Slash`, recomputed
/// the state, saw the hash agree, and accepted. A proposer could therefore name any bonded validator
/// and have every other validator confiscate that stake, honestly and deterministically, without a
/// single node asking whether the victim had done anything. The rule lived only on the producing side,
/// so the consuming side had nothing to check against it. Now both call this.
///
/// `BlockMetadata::slashable` — not `validation_failed` — is the signal, for the reason recorded on
/// that field: it is set only for a failure attributable to the *block*, so a node that merely could
/// not replay something locally does not thereby condemn its sender.
pub fn slashable_senders(
    justifications: &[rchain_models::block_metadata::BlockMetadata],
) -> BTreeSet<rchain_models::validator::Validator> {
    justifications
        .iter()
        .filter(|m| m.slashable)
        .map(|m| m.sender)
        .collect()
}

/// The validators a block slashes, read off its system deploys. A slash that *failed* during the
/// proposer's run carries no `SystemDeployData::Slash` (`ProcessedSystemDeploy::Failed` holds only an
/// error message), so this reads exactly the slashes that took effect.
pub fn slashed_validators(b: &BlockMessage) -> BTreeSet<rchain_models::validator::Validator> {
    b.state
        .system_deploys
        .iter()
        .filter_map(|sd| match sd {
            rchain_models::casper::protocol::casper_message::ProcessedSystemDeploy::Succeeded {
                system_deploy,
                ..
            } => match system_deploy {
                rchain_models::casper::protocol::casper_message::SystemDeployData::Slash(v) => {
                    Some(*v)
                }
                _ => None,
            },
            rchain_models::casper::protocol::casper_message::ProcessedSystemDeploy::Failed {
                ..
            } => None,
        })
        .collect()
}

// --- Effectful checks (depend on the block DAG) ------------------------------------------------

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use rchain_block_storage::block_store::BlockStore;
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_block_storage::dag::finalizer::Message;
use rchain_models::block_metadata::BlockMetadata;
use rchain_models::validator::Validator;

use crate::proto_util::{
    get_parent_metadatas_above_block_number, get_parents_metadata, max_block_number_metadata,
};
use crate::runtime_manager::RuntimeManager;

/// A block-validation outcome: `Ok(())` is valid, `Err(status)` is the invalid status (port of
/// `ValidBlockProcessing`).
pub type ValidBlockProcessing = Result<(), BlockStatus>;

/// Validate the block number against its justifications (port of `blockNumber`).
///
/// Two roles, deliberately separated (H1b, now enforced):
///
/// - **Every resolved parent must be *lower* than this block** — `Descends`
///   (`spec/Rchain/Casper/Dag.lean:Descends`) — failed or not. A failed block's recorded height is its
///   **claimed** `block_num` (`message_from_block_metadata`'s `height: block.block_num`), so without
///   this a block could name a failed parent far above itself, and the model's premise was false of a
///   state the port admitted. The witness is `dag.rs`'s
///   `h1b_a_failed_parent_above_the_childs_height_is_refused`, which was the *reproduction* of that
///   admitted violation before this check existed.
/// - **The maximum, by contrast, still skips failed justifications**: a failed block must not raise the
///   height its child claims. That is the oracle's own `if (!m.validationFailed)` and it stays.
///
/// The bound on failed parents is a **deliberate divergence from the Scala**, which skips them for the
/// height check too and so admits a block this refuses. The laws are the port's oracle, so the premise
/// the model needs is *guaranteed* here rather than assumed; the divergence is registered in §6 (a
/// peer sending such a block is refused — see the audit entry for the operator consequence).
///
/// Related, and unchanged: `neglected_invalid_block` refuses a block that justifies a failed **bonded**
/// validator's block (`h1b_a_justified_bonded_failed_block_is_refused_rather_than_forced`), so the
/// reachable route to a failed parent is the unbonded one.
pub async fn block_number(
    dag: &dyn BlockDagStorage,
    b: &BlockMessage,
) -> Result<ValidBlockProcessing, String> {
    let mut max_block_number = -1i64;
    for j in &b.justifications {
        let meta = dag
            .lookup(j)
            .await?
            .ok_or_else(|| format!("missing justification {}", j.to_hex()))?;
        // The descent bound, for *every* resolved parent (H1b).
        if i64::from(meta.block_num) >= i64::from(b.block_number) {
            return Ok(Err(BlockStatus::InvalidBlockNumber));
        }
        // The maximum, which skips failed justifications: they cannot raise the claimed height.
        if !meta.validation_failed {
            max_block_number = max_block_number.max(i64::from(meta.block_num));
        }
    }
    if max_block_number + 1 == i64::from(b.block_number) {
        Ok(Ok(()))
    } else {
        Ok(Err(BlockStatus::InvalidBlockNumber))
    }
}

/// Validate the sender's sequence number is one more than its latest justification's (port of
/// `sequenceNumber`).
pub async fn sequence_number(
    dag: &dyn BlockDagStorage,
    b: &BlockMessage,
) -> Result<ValidBlockProcessing, String> {
    let mut creator_latest_seq = -1i64;
    for j in &b.justifications {
        let meta = dag
            .lookup(j)
            .await?
            .ok_or_else(|| format!("missing justification {}", j.to_hex()))?;
        if meta.sender == b.sender {
            creator_latest_seq = creator_latest_seq.max(i64::from(meta.seq_num));
        }
    }
    if creator_latest_seq + 1 == i64::from(b.seq_num) {
        Ok(Ok(()))
    } else {
        Ok(Err(BlockStatus::InvalidSequenceNumber))
    }
}

/// Validate there is no justification regression (port of `justificationRegressions`).
pub async fn justification_regressions(
    dag: &dyn BlockDagStorage,
    b: &BlockMessage,
) -> Result<ValidBlockProcessing, String> {
    let valid = check_justification_regression(dag, b)
        .await?
        .unwrap_or(true);
    if valid {
        Ok(Ok(()))
    } else {
        Ok(Err(BlockStatus::JustificationRegression))
    }
}

async fn check_justification_regression(
    dag: &dyn BlockDagStorage,
    b: &BlockMessage,
) -> Result<Option<bool>, String> {
    let repr = dag.get_representation().await;
    let msg_map: &BTreeMap<BlockHash, Message<BlockHash, Validator>> =
        &repr.dag_message_state.msg_map;

    // `justifications.map(msgMap.get).sequence` — None if any is missing (see the Scala TODO).
    let justifications: Option<Vec<Message<BlockHash, Validator>>> = b
        .justifications
        .iter()
        .map(|j| msg_map.get(j).cloned())
        .collect();
    let justifications = match justifications {
        Some(js) => js,
        None => return Ok(None),
    };

    let prev_msg = match justifications.iter().find(|m| m.sender == b.sender) {
        Some(m) => m,
        None => return Ok(None),
    };

    let res = justifications.iter().all(|just| {
        let just_prev_msg = prev_msg
            .parents
            .iter()
            .filter_map(|p| msg_map.get(p))
            .find(|m| m.sender == just.sender);
        match just_prev_msg {
            Some(just_prev_msg) => just_prev_msg.seen.difference(&just.seen).next().is_none(),
            None => true,
        }
    });
    Ok(Some(res))
}

/// Validate that a block does not neglect an invalid-but-still-bonded justification (port of
/// `neglectedInvalidBlock`).
pub async fn neglected_invalid_block(
    dag: &dyn BlockDagStorage,
    b: &BlockMessage,
) -> Result<ValidBlockProcessing, String> {
    let mut justifications = Vec::new();
    for j in &b.justifications {
        if let Some(meta) = dag.lookup(j).await? {
            justifications.push(meta);
        }
    }
    let neglected = justifications
        .iter()
        .filter(|m| m.validation_failed)
        .map(|m| m.sender)
        .any(|v| {
            b.bonds
                .get(&v)
                .map(|&stake| i64::from(stake) > 0)
                .unwrap_or(false)
        });
    if neglected {
        Ok(Err(BlockStatus::NeglectedInvalidBlock))
    } else {
        Ok(Ok(()))
    }
}

/// Look up a block from the block store, failing if absent (port of `BlockStore.getUnsafe`).
async fn get_block_unsafe(
    block_store: &BlockStore,
    hash: &BlockHash,
) -> Result<BlockMessage, String> {
    let mut vals = block_store.get(&[*hash]).await?;
    vals.pop()
        .flatten()
        .ok_or_else(|| format!("missing block {}", hash.to_hex()))
}

/// Normalize a deploy signature to its low-S form so a high-S / low-S pair of the same ECDSA
/// signature are treated as the same deploy (signature malleability). `algorithm` is the deploy's
/// `sig_algorithm` (e.g. `"secp256k1"`). Delegates to the crypto crate's canonicalizer.
fn normalize_signature_low_s(algorithm: &str, signature: &[u8]) -> Vec<u8> {
    rchain_crypto::signatures::signatures_alg::normalize_signature_low_s(algorithm, signature)
}

/// Validate that no deploy with the same sig has been produced in the chain within the expiration
/// window (port of `repeatDeploy`).
pub async fn repeat_deploy(
    dag: &dyn BlockDagStorage,
    block_store: &BlockStore,
    block: &BlockMessage,
    expiration_threshold: i64,
) -> Result<ValidBlockProcessing, String> {
    let deploy_key_set: BTreeSet<Vec<u8>> = block
        .state
        .deploys
        .iter()
        .map(|d| normalize_signature_low_s(&d.deploy.sig_algorithm, &d.deploy.sig))
        .collect();

    let block_metadata = BlockMetadata::from_block(block);
    let init_parents = get_parents_metadata(dag, &block_metadata).await?;
    let max_block_number = max_block_number_metadata(&init_parents);
    let earliest_block_number = max_block_number + 1 - expiration_threshold;

    // Breadth-first traversal of the parent chain above the expiration horizon (port of
    // `DagOps.bfTraverseF(...).findF(...)`).
    let mut queue: VecDeque<BlockMetadata> = init_parents.into_iter().collect();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    while let Some(curr) = queue.pop_front() {
        if visited.contains(&curr.block_hash) {
            continue;
        }
        visited.insert(curr.block_hash);

        let b = get_block_unsafe(block_store, &curr.block_hash).await?;
        if b.state.deploys.iter().any(|d| {
            deploy_key_set.contains(&normalize_signature_low_s(
                &d.deploy.sig_algorithm,
                &d.deploy.sig,
            ))
        }) {
            return Ok(Err(BlockStatus::InvalidRepeatDeploy));
        }

        let parents =
            get_parent_metadatas_above_block_number(dag, &curr, earliest_block_number).await?;
        for p in parents {
            if !visited.contains(&p.block_hash) {
                queue.push_back(p);
            }
        }
    }
    Ok(Ok(()))
}

/// Validate that the block's bond cache matches the proof-of-stake contract's bonds at the post
/// state (port of `bondsCache`).
pub async fn bonds_cache(
    runtime: &RuntimeManager,
    block: &BlockMessage,
) -> Result<ValidBlockProcessing, String> {
    let tuplespace_hash = block.post_state_hash;
    let computed_bonds = runtime.compute_bonds(&tuplespace_hash).await?;
    if block.bonds == computed_bonds {
        Ok(Ok(()))
    } else {
        Ok(Err(BlockStatus::InvalidBondsCache))
    }
}

/// Compose the effectful + pure checks (port of `blockSummary`).
pub async fn block_summary(
    dag: &dyn BlockDagStorage,
    block_store: &BlockStore,
    block: &BlockMessage,
    shard_id: &str,
    expiration_threshold: i64,
    min_phlo_price: i64,
) -> Result<ValidBlockProcessing, String> {
    if let Err(status) = justification_regressions(dag, block).await? {
        return Ok(Err(status));
    }
    if let Err(status) = sequence_number(dag, block).await? {
        return Ok(Err(status));
    }
    if let Err(status) = block_number(dag, block).await? {
        return Ok(Err(status));
    }
    // Pure deploy checks — including `phlo_price`, which must be rejected *before* the expensive
    // replay so an economically-free deploy cannot force every validator to replay it (R27).
    let pure = [
        deploys_shard_identifier(block, shard_id),
        future_transaction(block),
        transaction_expiration(block, expiration_threshold),
        phlo_price(block, min_phlo_price),
        // `phlo_limit` belongs in this list for the reason `phlo_price` is here at all: both are
        // cheap tests on wire-supplied deploy data that must reject a block *before* the expensive
        // replay. For `phlo_limit` the stakes are higher — without it a peer's block reaches the
        // charge path with a negative limit (AUDIT C109).
        phlo_limit(block),
    ];
    for status in pure {
        if !status.is_valid() {
            return Ok(Err(status));
        }
    }
    repeat_deploy(dag, block_store, block, expiration_threshold).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use rchain_models::casper::protocol::casper_message::{
        DeployData, PCost, ProcessedDeploy, RholangState, SignedDeployData,
    };
    use rchain_models::validator::Validator;

    fn deploy(valid_after: i64, phlo_price: i64, shard_id: &str) -> ProcessedDeploy {
        ProcessedDeploy {
            deploy: SignedDeployData {
                data: DeployData {
                    attachments: Vec::new(),
                    term: "Nil".to_string(),
                    timestamp: 0,
                    phlo_price,
                    phlo_limit: 100,
                    valid_after_block_number: valid_after,
                    shard_id: shard_id.to_string(),
                },
                deployer: vec![],
                sig: vec![1],
                sig_algorithm: "secp256k1".to_string(),
            },
            cost: PCost { cost: 0 },
            deploy_log: vec![],
            is_failed: false,
            system_deploy_error: None,
        }
    }

    fn block() -> BlockMessage {
        BlockMessage {
            version: 1,
            shard_id: "root".to_string(),
            block_hash: BlockHash::new([0xab; 32]),
            block_number: 10.try_into().unwrap(),
            sender: Validator::new([0x11; 65]),
            seq_num: 0.try_into().unwrap(),
            pre_state_hash: rchain_models::block::state_hash::StateHash::new([1u8; 32]),
            post_state_hash: rchain_models::block::state_hash::StateHash::new([2u8; 32]),
            justifications: vec![],
            bonds: BTreeMap::new(),
            rejected_deploys: BTreeSet::new(),
            rejected_blocks: BTreeSet::new(),
            rejected_senders: BTreeSet::new(),
            state: RholangState {
                deploys: vec![],
                system_deploys: vec![],
            },
            sig_algorithm: "secp256k1".to_string(),
            sig: vec![1],
            timestamp: 0,
        }
    }

    #[test]
    fn version_and_format_checks() {
        let mut b = block();
        assert!(version(&b));
        b.version = 2;
        assert!(!version(&b));
        b.version = 1;
        assert!(format_of_fields(&b));
        b.sig = vec![];
        assert!(!format_of_fields(&b));
    }

    #[test]
    fn format_of_fields_rejects_non_ascii_shard_id() {
        let mut b = block();
        b.shard_id = "røøt".to_string();
        assert!(!format_of_fields(&b));
    }

    #[test]
    fn block_hash_detects_tampering() {
        let mut b = block();
        let h = hash_block(&b);
        b.block_hash = h;
        assert!(block_hash(&b));
        b.block_number = 999.try_into().unwrap();
        assert!(!block_hash(&b));
    }

    #[test]
    fn deploy_validators() {
        let mut b = block();
        b.state.deploys = vec![deploy(5, 10, "root")];
        assert_eq!(future_transaction(&b), BlockStatus::Valid);
        assert_eq!(transaction_expiration(&b, 100), BlockStatus::Valid);
        assert_eq!(deploys_shard_identifier(&b, "root"), BlockStatus::Valid);
        assert_eq!(phlo_price(&b, 10), BlockStatus::Valid);

        b.state.deploys = vec![deploy(20, 10, "root")];
        assert_eq!(future_transaction(&b), BlockStatus::ContainsFutureDeploy);

        b.state.deploys = vec![deploy(0, 10, "other")];
        assert_eq!(
            deploys_shard_identifier(&b, "root"),
            BlockStatus::InvalidDeployShardId
        );

        b.state.deploys = vec![deploy(5, 1, "root")];
        assert_eq!(phlo_price(&b, 10), BlockStatus::ContainsLowCostDeploy);
    }

    /// **The regression test for AUDIT C109**, and it is written against the *block* path on purpose.
    ///
    /// The mint was reachable two ways. A local deploy could not carry a negative limit — the ingress
    /// at `BlockApiImpl::deploy` checked for it — so a test placed there would have passed while the
    /// bug was live, which is the shape of a test that proves nothing. What *was* open is this path:
    /// a peer's block arrives, and the checks a validator runs on it (`deploys_shard_identifier`,
    /// `future_transaction`, `transaction_expiration`, `phlo_price`) all read the deploy data and none
    /// of them looked at the limit's sign. The block was accepted, replayed, and the negative charge
    /// credited its own author.
    ///
    /// Three assertions, because one is not enough to call it closed: the pure check rejects the
    /// block, the charge cannot be computed at all, and the arithmetic that used to invert is gone.
    #[test]
    fn a_negative_phlo_limit_cannot_reach_the_charge() {
        let mut negative = deploy(5, 10, "root");
        negative.deploy.data.phlo_limit = -100;
        assert_eq!(
            negative.deploy.data.total_phlo_charge(),
            None,
            "a negative limit must not produce a charge — this is the value that used to be \
             subtracted from the deployer's vault, i.e. added to it"
        );

        let mut b = block();
        b.state.deploys = vec![negative];
        assert_eq!(
            phlo_limit(&b),
            BlockStatus::InvalidPhloLimit,
            "a peer's block carrying a negative phlo limit must be refused by name, before replay"
        );
        assert_eq!(
            phlo_price(&b, 10),
            BlockStatus::Valid,
            "the rejection is the limit's, not the price's — the two checks must not be the same check"
        );

        // The control: the same deploy with a non-negative limit is valid, so the test above is
        // failing for the sign and not because the block builder produces something invalid anyway.
        let mut positive = deploy(5, 10, "root");
        positive.deploy.data.phlo_limit = 0;
        b.state.deploys = vec![positive];
        assert_eq!(phlo_limit(&b), BlockStatus::Valid);
    }

    /// **The regression test for AUDIT C110.** A `Slash` is a system deploy: unsigned by its victim,
    /// it moves that victim's whole bond to the Coop vault, and every validator re-executes it during
    /// replay. The rule that makes one justified — the victim is the sender of a `slashable`
    /// justification of the block — used to exist only on the proposer's side, so the validators that
    /// *executed* the punishment had nothing to check it against.
    ///
    /// This pins the two halves that make the receiver's check possible: which validators a block
    /// actually slashes (read off its system deploys), and which its evidence holds responsible (read
    /// off this node's own metadata). The subset test between them is the check itself.
    #[test]
    fn a_slash_is_justified_only_by_a_slashable_justification_from_its_victim() {
        use rchain_models::block_metadata::BlockMetadata;
        use rchain_models::casper::protocol::casper_message::{
            ProcessedSystemDeploy, SystemDeployData,
        };

        let offender = Validator::new([0x22; 65]);
        let innocent = Validator::new([0x33; 65]);

        let mut b = block();
        b.state.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash(offender),
        }];
        assert_eq!(slashed_validators(&b), BTreeSet::from([offender]));

        // Evidence that holds `offender` responsible: the slash is covered.
        let mut meta = BlockMetadata::from_block(&b);
        meta.sender = offender;
        meta.slashable = true;
        let justified = slashable_senders(&[meta.clone()]);
        assert!(
            slashed_validators(&b).is_subset(&justified),
            "a slash of the validator its evidence condemns must be covered"
        );

        // The attack this check exists for: the block slashes `innocent`, and nothing in the
        // justifications holds `innocent` responsible. Before this check, the block was accepted on
        // the strength of its post-state hash alone.
        b.state.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash(innocent),
        }];
        assert!(
            !slashed_validators(&b).is_subset(&justified),
            "slashing a validator no justification holds responsible must be refused — this is the \
             stake-confiscation path a proposer could take against any bonded peer"
        );

        // `validation_failed` alone does not authorize: a node that could not replay a block locally
        // says nothing about its sender, so that flag must not make a slash justified.
        let mut merely_failed = BlockMetadata::from_block(&b);
        merely_failed.sender = innocent;
        merely_failed.validation_failed = true;
        merely_failed.slashable = false;
        assert!(
            !slashed_validators(&b).is_subset(&slashable_senders(&[merely_failed])),
            "an unattributable local failure must not license a slash"
        );
    }
}

#[cfg(test)]
mod effectful_tests {
    use super::*;
    use async_trait::async_trait;
    use rchain_block_storage::dag::codecs::{BlockHashCodec, BlockMessageCodec};
    use rchain_block_storage::dag::dag_storage::DeployId;
    use rchain_block_storage::dag::message_state::DagMessageState;
    use rchain_block_storage::dag::representation::DagRepresentation;
    use rchain_models::block_metadata::BlockMetadata;
    use rchain_models::casper::protocol::casper_message::{
        DeployData, PCost, ProcessedDeploy, SignedDeployData,
    };
    use rchain_shared::store::InMemoryKeyValueStore;
    use rchain_shared::typed_store::KeyValueTypedStoreCodec;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn hash(byte: u8) -> BlockHash {
        let mut bytes = [0u8; 32];
        bytes[0] = byte;
        BlockHash::new(bytes)
    }

    fn meta(
        hash: BlockHash,
        block_num: i64,
        sender_byte: u8,
        seq: i64,
        failed: bool,
    ) -> BlockMetadata {
        BlockMetadata {
            block_hash: hash,
            block_num: rchain_shared::refined::BlockHeight::try_from(block_num).unwrap(),
            sender: Validator::new([sender_byte; 65]),
            seq_num: rchain_shared::refined::SeqNum::try_from(seq).unwrap(),
            justifications: BTreeSet::new(),
            bonds_map: BTreeMap::new(),
            validated: true,
            validation_failed: failed,
            slashable: false,
            fringe: BTreeSet::new(),
            fringe_state_hash: rchain_models::block::state_hash::StateHash::new([0u8; 32]),
            member_of_fringe: None,
        }
    }

    struct MockDag {
        metadata: BTreeMap<BlockHash, BlockMetadata>,
        representation: DagRepresentation,
    }

    #[async_trait]
    impl BlockDagStorage for MockDag {
        async fn get_representation(&self) -> Arc<DagRepresentation> {
            Arc::new(self.representation.clone())
        }
        async fn insert(&self, _m: BlockMetadata, _b: BlockMessage) -> Result<(), String> {
            Ok(())
        }
        async fn lookup(&self, h: &BlockHash) -> Result<Option<BlockMetadata>, String> {
            Ok(self.metadata.get(h).cloned())
        }
        async fn lookup_by_deploy_id(&self, _d: &DeployId) -> Result<Option<BlockHash>, String> {
            Ok(None)
        }
        async fn add_deploy(&self, _d: SignedDeployData) -> Result<(), String> {
            Ok(())
        }
        async fn pooled_deploys(&self) -> Result<BTreeMap<DeployId, SignedDeployData>, String> {
            Ok(BTreeMap::new())
        }
        async fn contains_deploy_in_pool(&self, _d: &DeployId) -> Result<bool, String> {
            Ok(false)
        }
    }

    fn mock(metadata: BTreeMap<BlockHash, BlockMetadata>) -> MockDag {
        MockDag {
            metadata,
            representation: DagRepresentation {
                dag_set: Arc::new(BTreeSet::new()),
                child_map: Arc::new(BTreeMap::new()),
                height_map: Arc::new(BTreeMap::new()),
                dag_message_state: DagMessageState::empty(),
                fringe_states: BTreeMap::new(),
            },
        }
    }

    fn block(
        sender_byte: u8,
        block_num: i64,
        seq: i64,
        justifications: Vec<BlockHash>,
    ) -> BlockMessage {
        BlockMessage {
            version: 1,
            shard_id: "root".to_string(),
            block_hash: hash(0xee),
            block_number: rchain_shared::refined::BlockHeight::try_from(block_num).unwrap(),
            sender: Validator::new([sender_byte; 65]),
            seq_num: rchain_shared::refined::SeqNum::try_from(seq).unwrap(),
            pre_state_hash: rchain_models::block::state_hash::StateHash::new([1u8; 32]),
            post_state_hash: rchain_models::block::state_hash::StateHash::new([2u8; 32]),
            justifications,
            bonds: BTreeMap::new(),
            rejected_deploys: BTreeSet::new(),
            rejected_blocks: BTreeSet::new(),
            rejected_senders: BTreeSet::new(),
            state: rchain_models::casper::protocol::casper_message::RholangState::default(),
            sig_algorithm: "secp256k1".to_string(),
            sig: vec![1],
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn block_number_must_be_parent_max_plus_one() {
        let parent = hash(1);
        let dag = mock(BTreeMap::from([(parent, meta(parent, 4, 1, 0, false))]));
        let b = block(2, 5, 0, vec![parent]);
        assert_eq!(block_number(&dag, &b).await.unwrap(), Ok(()));

        let bad = block(2, 6, 0, vec![parent]);
        assert_eq!(
            block_number(&dag, &bad).await.unwrap(),
            Err(BlockStatus::InvalidBlockNumber)
        );
    }

    #[tokio::test]
    async fn sequence_number_must_be_creator_latest_plus_one() {
        let parent = hash(1);
        let dag = mock(BTreeMap::from([(parent, meta(parent, 4, 1, 2, false))]));
        let b = block(1, 5, 3, vec![parent]);
        assert_eq!(sequence_number(&dag, &b).await.unwrap(), Ok(()));

        let bad = block(1, 5, 4, vec![parent]);
        assert_eq!(
            sequence_number(&dag, &bad).await.unwrap(),
            Err(BlockStatus::InvalidSequenceNumber)
        );
    }

    #[tokio::test]
    async fn neglected_invalid_block_detects_bonded_invalid_justification() {
        let invalid = hash(1);
        let dag = mock(BTreeMap::from([(invalid, meta(invalid, 0, 1, 0, true))]));
        let mut b = block(2, 1, 0, vec![invalid]);
        b.bonds
            .insert(Validator::new([1u8; 65]), 100.try_into().unwrap());
        assert_eq!(
            neglected_invalid_block(&dag, &b).await.unwrap(),
            Err(BlockStatus::NeglectedInvalidBlock)
        );

        b.bonds.clear();
        assert_eq!(neglected_invalid_block(&dag, &b).await.unwrap(), Ok(()));
    }

    fn deploy(sig: u8) -> ProcessedDeploy {
        ProcessedDeploy {
            deploy: SignedDeployData {
                data: DeployData {
                    attachments: Vec::new(),
                    term: "Nil".to_string(),
                    timestamp: 0,
                    phlo_price: 1,
                    phlo_limit: 1,
                    valid_after_block_number: 0,
                    shard_id: "root".to_string(),
                },
                deployer: vec![],
                sig: vec![sig],
                sig_algorithm: "secp256k1".to_string(),
            },
            cost: PCost { cost: 0 },
            deploy_log: vec![],
            is_failed: false,
            system_deploy_error: None,
        }
    }

    async fn block_store(blocks: Vec<BlockMessage>) -> BlockStore {
        let store: BlockStore = Arc::new(KeyValueTypedStoreCodec::new(
            Arc::new(tokio::sync::Mutex::new(Box::new(
                InMemoryKeyValueStore::default(),
            ))),
            Arc::new(BlockHashCodec),
            Arc::new(BlockMessageCodec),
        ));
        let pairs: Vec<(BlockHash, BlockMessage)> =
            blocks.into_iter().map(|b| (b.block_hash, b)).collect();
        store.put(&pairs).await.unwrap();
        store
    }

    #[tokio::test]
    async fn repeat_deploy_detects_duplicate_sig_in_parent_chain() {
        let genesis = hash(1);
        let parent = hash(2);
        let dag = mock(BTreeMap::from([
            (genesis, meta(genesis, 0, 1, 0, false)),
            (parent, meta(parent, 1, 1, 1, false)),
        ]));

        let mut genesis_block = block(1, 0, 0, vec![]);
        genesis_block.block_hash = genesis;
        genesis_block.state.deploys = vec![deploy(9)];
        let mut parent_block = block(1, 1, 1, vec![genesis]);
        parent_block.block_hash = parent;
        parent_block.state.deploys = vec![deploy(1)];
        let store = block_store(vec![genesis_block, parent_block]).await;

        // Current block reuses the parent's deploy sig [1].
        let mut current = block(1, 2, 2, vec![parent]);
        current.state.deploys = vec![deploy(1)];
        assert_eq!(
            repeat_deploy(&dag, &store, &current, 100).await.unwrap(),
            Err(BlockStatus::InvalidRepeatDeploy)
        );

        // Current block with a fresh deploy sig is valid.
        let mut current = block(1, 2, 2, vec![parent]);
        current.state.deploys = vec![deploy(7)];
        assert_eq!(
            repeat_deploy(&dag, &store, &current, 100).await.unwrap(),
            Ok(())
        );
    }
}
