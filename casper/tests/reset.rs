//! The reset's acting half, driven where it is decided (#320, F-U5-01's acting half, register C274).
//!
//! **What this is for.** The engine half was pinned by reading while the rest of the unit was tested:
//! the loop's third exit (`node_launch::apply`'s `select!`) has no harness — only `node_runtime` calls
//! `apply`, and building the fixture it would need (a transport, an importer, an exporter, a
//! `ShardSpec`, a `NodeRunning`) is a project rather than a test. So the arm's *body* was extracted into
//! `engine::reset::act_on_reset_request`, which takes the four things the decision actually reads — the
//! switch, the two stores and the runtime — and this drives *that*.
//!
//! What it therefore pins: the request is consumed exactly once, the switch gates the act, the act
//! rewinds the DAG and purges the bodies it dropped, and the anchor survives both. What it deliberately
//! does not pin: the five lines of `select!` around the call, which are read rather than run — said out
//! loud here rather than left as an implication that the loop is covered.
//!
//! **The anchor is real.** The fixture uses the runtime manager's own root as the anchor block's
//! post-state, so the runtime reset is the ordinary one (`RhoRuntime::reset` on a root the history
//! knows) and not a fabricated hash — which is what makes the whole act run rather than stopping at the
//! first step.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rchain_block_storage::block_store::BlockStore;
use rchain_block_storage::dag::codecs::{
    BlockHashCodec, BlockMetadataCodec, SignedDeployDataCodec,
};
use rchain_block_storage::dag::dag_storage::{BlockDagStorage, DeployId};
use rchain_casper::block_metadata_store::BlockMetadataStore;
use rchain_casper::dag::BlockDagKeyValueStorage;
use rchain_casper::engine::reset::act_on_reset_request;
use rchain_casper::interpreter_util::{reset_request, take_reset_request, ResetRequest};
use rchain_models::block::state_hash::StateHash;
use rchain_models::block_hash::BlockHash;
use rchain_models::block_metadata::{BlockMetadata, SlashSeverity};
use rchain_models::casper::protocol::casper_message::{
    BlockMessage, RholangState, SignedDeployData,
};
use rchain_models::validator::Validator;
use rchain_shared::log::{LogSource, NopLog};
use rchain_shared::refined::BlockHeight;
use rchain_shared::store::InMemoryKeyValueStore;
use rchain_shared::typed_store::{
    BytesCodec, KeyValueTypedStore, KeyValueTypedStoreCodec, SharedStore,
};

fn hash(byte: u8) -> BlockHash {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    BlockHash::new(bytes)
}

/// A block at `hash` whose post-state is `post_state_hash` — so a fixture can make the anchor carry
/// the runtime's *own* root, which is what lets the reset's last step run instead of failing at it.
fn block(hash: BlockHash, post_state_hash: StateHash) -> BlockMessage {
    BlockMessage {
        version: 1,
        shard_id: "/root".to_string(),
        block_hash: hash,
        block_number: BlockHeight::zero(),
        sender: Validator::new([0u8; 65]),
        seq_num: 0.try_into().unwrap(),
        pre_state_hash: StateHash::new([0u8; 32]),
        post_state_hash,
        justifications: vec![],
        bonds: BTreeMap::new(),
        rejected_deploys: BTreeSet::new(),
        rejected_blocks: BTreeSet::new(),
        rejected_senders: BTreeSet::new(),
        state: RholangState::default(),
        sig_algorithm: "secp256k1".to_string(),
        sig: vec![],
        timestamp: 0,
    }
}

fn meta(hash: BlockHash, parents: &[BlockHash], block_num: i64, seq: i64) -> BlockMetadata {
    BlockMetadata {
        block_hash: hash,
        block_num: BlockHeight::try_from(block_num).unwrap(),
        sender: Validator::new([0u8; 65]),
        seq_num: seq.try_into().unwrap(),
        justifications: parents.iter().copied().collect(),
        bonds_map: BTreeMap::new(),
        validated: true,
        validation_failed: false,
        slashable: false,
        failure_cause: None,
        slash_severity: SlashSeverity::Unspecified,
        restore_attempts: 0,
        fringe: BTreeSet::new(),
        fringe_state_hash: StateHash::new([0u8; 32]),
        member_of_fringe: None,
    }
}

/// An empty DAG over in-memory stores. `node_running.rs`'s and `dag.rs`'s test modules each build the
/// same storage for themselves (the helper is `#[cfg(test)]` and so unreachable across modules); this is
/// the third copy, and the reason the duplication is the recorded practice is in `build_dag`'s own note
/// there: widening it into a production constructor would put a test seam in the engine's API.
async fn build_dag() -> Arc<BlockDagKeyValueStorage> {
    let fresh = || -> SharedStore {
        Arc::new(tokio::sync::Mutex::new(Box::new(
            InMemoryKeyValueStore::default(),
        )))
    };
    let metadata = Arc::new(
        BlockMetadataStore::create(Arc::new(KeyValueTypedStoreCodec::new(
            fresh(),
            Arc::new(BlockHashCodec),
            Arc::new(BlockMetadataCodec),
        )))
        .await
        .expect("metadata store"),
    );
    let deploy_index: Arc<dyn KeyValueTypedStore<DeployId, BlockHash>> = Arc::new(
        KeyValueTypedStoreCodec::new(fresh(), Arc::new(BytesCodec), Arc::new(BlockHashCodec)),
    );
    let deploy_store: Arc<dyn KeyValueTypedStore<DeployId, SignedDeployData>> =
        Arc::new(KeyValueTypedStoreCodec::new(
            fresh(),
            Arc::new(BytesCodec),
            Arc::new(SignedDeployDataCodec),
        ));
    Arc::new(
        BlockDagKeyValueStorage::create(metadata, deploy_index, deploy_store)
            .await
            .expect("dag storage"),
    )
}

async fn build_block_store() -> BlockStore {
    let manager = rchain_shared::store_manager::InMemoryStoreManager::default();
    rchain_block_storage::block_store::create(&manager)
        .await
        .expect("block store")
}

async fn has(store: &BlockStore, h: BlockHash) -> bool {
    store
        .contains(&[h])
        .await
        .expect("the store answers")
        .first()
        .copied()
        .unwrap_or(false)
}

/// **The acting half, end to end at the level a test can reach.** One test rather than three, because
/// the reset request is a process-wide cell: the first request wins, so a second test's request would
/// be refused while this one's was pending — the same reason the signal's own contract test is one.
#[tokio::test]
async fn a_reset_request_rewinds_the_stores_when_the_switch_is_on_and_does_nothing_when_it_is_off()
{
    // Whatever a previous run left in the cell: this test owns it from here.
    let _ = take_reset_request();

    let dag = build_dag().await;
    let block_store = build_block_store().await;
    let manager = Arc::new(common::build_runtime_manager().await);
    // **A root the runtime knows**, so the reset's last step is the ordinary one.
    let known_root = manager.get_history_repo().root();
    let root_state = StateHash::new(known_root.to_byte_array());

    // A chain of four: heights 0..3, the tip above the anchor we will name.
    let mut parents: Vec<BlockHash> = Vec::new();
    for n in 0..4i64 {
        let h = hash(n as u8);
        // The block at height 1 carries the runtime's root; the rest are ordinary.
        let post = if n == 1 {
            root_state
        } else {
            StateHash::new([0u8; 32])
        };
        dag.insert(meta(h, &parents, n, n), block(h, post))
            .await
            .expect("the chain inserts");
        block_store
            .put(&[(h, block(h, post))])
            .await
            .expect("the body is stored");
        parents = vec![h];
    }

    let log: Arc<dyn rchain_shared::log::Log> = Arc::new(NopLog);

    let anchor = hash(1);
    let anchor_height = BlockHeight::try_from(1).unwrap();
    let refused = hash(3);
    let request = ResetRequest {
        anchor,
        anchor_height,
        blocks_above: 2,
        refused,
    };

    // --- the switch off --------------------------------------------------------------------------
    // First half: a node whose operator has not opted in must *not* touch its stores, and must consume
    // the request rather than leaving it pending for ever.
    assert!(
        rchain_casper::interpreter_util::request_reset(request.clone()),
        "the request is raised"
    );
    let off = act_on_reset_request(
        false,
        &(dag.clone() as Arc<dyn BlockDagStorage>),
        &block_store,
        &manager,
        &log,
        LogSource::new("test"),
    )
    .await
    .expect("a gated reset is not an error");
    assert!(off.is_none(), "the switch is off, so nothing acted");
    assert_eq!(
        reset_request(),
        None,
        "and the request was consumed, not left pending"
    );
    assert!(
        has(&block_store, refused).await,
        "the tip's body is still there: nothing was rewound"
    );

    // --- the switch on ---------------------------------------------------------------------------
    assert!(
        rchain_casper::interpreter_util::request_reset(request.clone()),
        "a second request is accepted once the first is consumed"
    );
    let acted = act_on_reset_request(
        true,
        &(dag.clone() as Arc<dyn BlockDagStorage>),
        &block_store,
        &manager,
        &log,
        LogSource::new("test"),
    )
    .await
    .expect("the reset runs over healthy stores and a known root");
    assert_eq!(
        acted.map(|r| r.anchor),
        Some(anchor),
        "the caller is told which anchor to catch up from"
    );

    let repr = dag.get_representation().await;
    assert!(!repr.contains(&refused), "the DAG is at the anchor");
    assert!(
        repr.contains(&anchor),
        "and the anchor survives — it is the state replayed from"
    );
    drop(repr);
    assert!(
        !has(&block_store, refused).await,
        "the dropped body is gone, so it can be neither served nor re-injected"
    );
    assert!(
        has(&block_store, anchor).await,
        "the anchor's own body stays — the reset keeps what it rewinds to"
    );
    assert_eq!(
        reset_request(),
        None,
        "taking the request is not undone by acting on it"
    );
}
