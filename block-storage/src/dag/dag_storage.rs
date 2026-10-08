//! Block DAG storage interface.
//!
//! Mirrors `block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagStorage.scala`. The
//! concrete `BlockDagKeyValueStorage` is casper-owned in Scala, so only the trait is ported here.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use rchain_models::block_hash::BlockHash;
use rchain_models::block_metadata::BlockMetadata;
use rchain_models::casper::protocol::casper_message::{BlockMessage, SignedDeployData};

use super::representation::DagRepresentation;

/// A deploy id (the Scala `BlockDagStorage.DeployId = ByteString`).
pub type DeployId = Vec<u8>;

/// What a node knows about one deployer key: a block that includes a deploy it signed, and the height
/// from which that knowledge is complete.
///
/// `indexed_from` is the lowest height at and above which **every** block this node has inserted was
/// indexed. It is never below the lowest block the node holds: a node that started indexing on an
/// existing chain has not read the blocks below its marker, and a node that joined by last-finalized-
/// state sync never stored the blocks below the fringe. So `block: None` with `indexed_from > 0` means
/// "not in any block from `indexed_from` up" — never "this key has never signed". A node that holds
/// and has indexed every block from genesis reports `0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployerLookup {
    pub block: Option<BlockHash>,
    pub indexed_from: i64,
}

/// The block DAG storage interface (port of `BlockDagStorage[F]`). The concrete implementation is
/// the `casper` crate's `BlockDagKeyValueStorage`.
#[async_trait]
pub trait BlockDagStorage: Send + Sync {
    /// The current DAG representation, **shared rather than copied**.
    ///
    /// A `DagRepresentation` owns every message, and each message's `seen` is its whole ancestry, so
    /// it is Θ(N²) in the number of blocks. Returning it by value made every caller pay that copy —
    /// ~35 call sites, including every per-block path and every peer-request handler, with the read
    /// lock held for the whole copy; on a node serving a syncing peer that was the difference between
    /// a flat curve and ~0.86 GiB per block. Callers read fields through `Deref`, so the change is
    /// invisible to them; the writer takes the copy at most once per insert via `Arc::make_mut`.
    async fn get_representation(&self) -> Arc<DagRepresentation>;

    /// **The equivocating blocks this node has refused**, as serialized blocks keyed by the sender that
    /// equivocated (AUDIT C200).
    ///
    /// The H-1 gate refuses such a block *before any write*, so nothing else in the tree holds it — and
    /// a proposer cannot slash for an equivocation it cannot show. Recording it here gives the evidence
    /// somewhere to live that is **not** consensus state until a block actually carries it, and reading
    /// it is what lets a proposer attach a proof every other node can check for itself.
    ///
    /// The default is empty, and that is a statement rather than a stub: a storage that does not record
    /// them has none, and only the real implementation can.
    async fn recorded_equivocations(&self) -> Vec<(rchain_models::validator::Validator, Vec<u8>)> {
        Vec::new()
    }

    async fn insert(
        &self,
        block_metadata: BlockMetadata,
        block: BlockMessage,
    ) -> Result<(), String>;

    /// **Update the record of a block the DAG already holds** — the write the restoring rule needs, and
    /// `insert` is not it (AUDIT C193).
    ///
    /// `insert`'s contract is to *add* a block, and a re-insert of a hash it already holds is a no-op —
    /// which is right for that contract and wrong for this one. `restore_divergent_justifications`
    /// re-validates a stored justification and writes back the **same block's** record with
    /// `validation_failed` cleared; through `insert` that returned `Ok(())` without writing, so the rule
    /// logged *"cleared the failure record"* while the store kept the failure and the in-memory
    /// representation was never touched. The rule has never cleared anything.
    ///
    /// An update also has to undo the two structures that excluded the block while it was failed: the
    /// height map (which does not count a failed block) and `latest_msgs` (H-2, so a failed block cannot
    /// be a parent — and therefore cannot advance the proposer's arithmetic, which is the sequence-number
    /// wedge that made this visible).
    ///
    /// **The default refuses rather than silently doing nothing.** A no-op default is the defect this
    /// method exists to fix — a write that returns `Ok` and changes nothing is precisely what the
    /// restoring rule has been doing — so a backend that cannot update says so, and the failure is
    /// visible in the log that names the block.
    async fn update_metadata(&self, block_metadata: BlockMetadata) -> Result<(), String> {
        Err(format!(
            "this DAG storage cannot update the record of {} (no `update_metadata` impl)",
            block_metadata.block_hash.to_hex()
        ))
    }

    async fn lookup(&self, block_hash: &BlockHash) -> Result<Option<BlockMetadata>, String>;

    /// Look up a block hash by the deploy id included in the DAG.
    async fn lookup_by_deploy_id(&self, deploy_id: &DeployId) -> Result<Option<BlockHash>, String>;

    /// A block containing a deploy signed by the key whose `blake2b256` hash is `deployer_hash`, from
    /// the deployer index — the first such block this node inserted. It takes the hash, not the key,
    /// so a wallet checking a key that has never signed need not reveal that key to the node. No Scala counterpart: it serves the wallet's
    /// "has this key been revealed?" check (quantum key hygiene), which otherwise has to scan every
    /// block. The default refuses rather than answering `None`: a backend without the index cannot say
    /// a key is unseen.
    async fn lookup_by_deployer(&self, _deployer_hash: &[u8]) -> Result<DeployerLookup, String> {
        Err("this DAG storage keeps no deployer index".to_string())
    }

    /// Add a deploy to the (unprocessed) deploy pool.
    async fn add_deploy(&self, deploy: SignedDeployData) -> Result<(), String>;

    async fn pooled_deploys(&self) -> Result<BTreeMap<DeployId, SignedDeployData>, String>;

    async fn contains_deploy_in_pool(&self, deploy_id: &DeployId) -> Result<bool, String>;
}
