//! **C215 over the real storage**: does the merge's answer depend on the order a node received the same
//! blocks in? Two DAGs, one block set, two arrival orders — and the only difference is the one the row's
//! own note said could not exist.
//!
//! The row's note (`spec/findings.tsv`, C215) reasons that "delivery order alone is not a difference
//! between two nodes — only different *content* is", because `child_map`, `msg_map` and the height map are
//! `BTreeMap`/`BTreeSet` over the same set, and `latest_msgs` is order-sensitive only on a tie, which the
//! H-1 gate refuses before any write. That reasoning is right about those four, and it misses a fifth map
//! in the merge's input: **`fringe_states`**.
//!
//! `FringeData` is keyed by `fringe_hash_of(fringe_set)` and carries **per-block** values —
//! `state_hash`, `rejected_deploys`, `rejected_blocks`, `rejected_senders` (`models/src/fringe_data.rs`),
//! and its `Hash` impl hashes only the key. So the map's value is not a function of its key: two blocks
//! that finalise **the same fringe set** but disagree about that set's state are the *same key*, and the
//! write is last-write-wins (`casper/src/dag.rs`). Two nodes that received those two blocks in different
//! orders therefore hold different `fringe_states[K]`, hold the same everything else, and read that map
//! unconditionally — for the merged base state (`multi_parent_casper.rs::get_pre_state_for_parents`) and
//! for the rejection sets (`merging.rs::rejections_for`).
//!
//! That is not exotic. It is *the incident*: four blocks at one height, the same justification, four
//! different state hashes, and each node rejecting the others' on
//! `state-hash disagreement on pre-state: block #104` (`spec/audit/evidence/te-1-2026-10-09-four-divergent-heads.md`).
//!
//! **What this file measures, and what it does not.** It measures the storage: that two arrival orders
//! leave two different caches for one block set. The second link in the chain — that a different cache
//! yields different rejection sets for one scope — is the unit test
//! `merging::tests::two_caches_of_one_block_set_reject_differently`, because `MergeScope::merge` needs a
//! `RhoHistoryRepository`. Composed, the two are the defect. Neither is a model of the other's subject:
//! this one drives the production `insert`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rchain_block_storage::dag::codecs::{
    Blake2b256HashCodec, BlockHashCodec, BlockMetadataCodec, FringeDataCodec, SignedDeployDataCodec,
};
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_casper::block_metadata_store::BlockMetadataStore;
use rchain_casper::dag::BlockDagKeyValueStorage;
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_models::block::state_hash::StateHash;
use rchain_models::block_hash::BlockHash;
use rchain_models::block_metadata::BlockMetadata;
use rchain_models::casper::protocol::casper_message::{BlockMessage, RholangState};
use rchain_models::fringe_data::FringeData;
use rchain_models::validator::Validator;
use rchain_shared::refined::{BlockHeight, SeqNum};
use rchain_shared::store::{InMemoryKeyValueStore, KeyValueStore};
use rchain_shared::typed_store::{
    BytesCodec as RawBytesCodec, KeyValueTypedStore, KeyValueTypedStoreCodec,
};

type Shared = Arc<tokio::sync::Mutex<Box<dyn KeyValueStore + Send + Sync>>>;

fn in_memory() -> Shared {
    Arc::new(tokio::sync::Mutex::new(Box::new(
        InMemoryKeyValueStore::default(),
    )))
}

/// The storage a node builds — the same four stores `casper/src/dag.rs`'s own tests construct, so
/// everything below goes through the production `insert`.
async fn build_storage() -> Arc<BlockDagKeyValueStorage> {
    let metadata_store = Arc::new(
        BlockMetadataStore::create(Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(BlockHashCodec),
            Arc::new(BlockMetadataCodec),
        )))
        .await
        .expect("metadata store"),
    );
    let fringe_store: Arc<dyn KeyValueTypedStore<Blake2b256Hash, FringeData>> =
        Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(Blake2b256HashCodec),
            Arc::new(FringeDataCodec),
        ));
    let deploy_index: Arc<dyn KeyValueTypedStore<Vec<u8>, BlockHash>> =
        Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(RawBytesCodec),
            Arc::new(BlockHashCodec),
        ));
    let deploy_store: Arc<
        dyn KeyValueTypedStore<
            Vec<u8>,
            rchain_models::casper::protocol::casper_message::SignedDeployData,
        >,
    > = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(RawBytesCodec),
        Arc::new(SignedDeployDataCodec),
    ));
    Arc::new(
        BlockDagKeyValueStorage::create(metadata_store, fringe_store, deploy_index, deploy_store)
            .await
            .expect("dag storage"),
    )
}

fn hash(byte: u8) -> BlockHash {
    BlockHash::new([byte; 32])
}

/// A block, unsigned: the DAG's own gate checks `(sender, seq_num)` and its dependencies, not signatures,
/// and this file is about the order blocks arrive in rather than about who signed them.
fn block(id: u8, sender: u8, seq: i64, height: i64, parents: &[BlockHash]) -> BlockMessage {
    BlockMessage {
        version: 1,
        shard_id: "root".to_string(),
        block_hash: hash(id),
        block_number: BlockHeight::try_from(height).expect("a height"),
        sender: Validator::new([sender; 65]),
        seq_num: SeqNum::try_from(seq).expect("a sequence number"),
        pre_state_hash: StateHash::new([id; 32]),
        post_state_hash: StateHash::new([id; 32]),
        justifications: parents.to_vec(),
        bonds: BTreeMap::new(),
        rejected_deploys: BTreeSet::new(),
        rejected_blocks: BTreeSet::new(),
        rejected_senders: BTreeSet::new(),
        state: RholangState::default(),
        sig_algorithm: "secp256k1".to_string(),
        sig: Vec::new(),
        timestamp: 0,
    }
}

/// The metadata a *receiving* node records, with the two fields that are the node's own derived values
/// set explicitly: `fringe` (the finalised set this block finalises) and `fringe_state_hash` (the state
/// at that set). `BlockMetadata::from_block` leaves both empty, because on the wire a block carries
/// neither — they are what the receiver computes from its own view, which is exactly why two receivers
/// can compute them differently.
fn meta(b: &BlockMessage, fringe: &[BlockHash], fringe_state: u8) -> BlockMetadata {
    let mut m = BlockMetadata::from_block(b);
    m.fringe = fringe.iter().copied().collect();
    m.fringe_state_hash = StateHash::new([fringe_state; 32]);
    m
}

async fn insert(dag: &BlockDagKeyValueStorage, m: BlockMetadata, b: BlockMessage) {
    dag.insert(m, b).await.expect("the block inserts");
}

/// **Two arrival orders, one block set, two different caches.**
///
/// `p`, `x` and `y` all justify the genesis block `g` and all carry `fringe = {g}` — the same fringe
/// *set*, so the same key `K` — while disagreeing about that set's state: `p` and `x` and `y` each claim a
/// different `fringe_state_hash`. They are three blocks at one height by three different senders, which is
/// ordinary multi-proposer behaviour, not an offence.
///
/// The two nodes insert the same four blocks in opposite orders. The assertions are ordered so the test
/// cannot pass vacuously: the four maps that *are* functions of the block set must be **equal** (that is
/// the premise — "the same justifications"), and the one map that is not must **differ** at `K`. If the
/// second assertion fails, this file says nothing and C215's mechanism is refuted rather than reproduced.
#[tokio::test]
async fn two_arrival_orders_of_one_block_set_leave_different_fringe_caches() {
    let g = block(1, 1, 0, 0, &[]);
    // One sender, ascending sequence numbers, as a real chain of one proposer's blocks looks.
    let p = block(2, 1, 1, 1, &[hash(1)]);
    let x = block(3, 2, 0, 1, &[hash(1)]);
    let y = block(4, 3, 0, 1, &[hash(1)]);

    // The genesis block finalises nothing, so its fringe is empty — and it must be, because `insert`
    // computes the fringe from the message map *before* the block itself is in it.
    let g_meta = meta(&g, &[], 11);
    let p_meta = meta(&p, &[hash(1)], 21);
    let x_meta = meta(&x, &[hash(1)], 31);
    let y_meta = meta(&y, &[hash(1)], 41);

    let key = FringeData::fringe_hash_of(&[hash(1)].into_iter().collect::<BTreeSet<_>>());

    let first = build_storage().await;
    insert(&first, g_meta.clone(), g.clone()).await;
    insert(&first, p_meta.clone(), p.clone()).await;
    insert(&first, x_meta.clone(), x.clone()).await;
    insert(&first, y_meta.clone(), y.clone()).await;

    let second = build_storage().await;
    insert(&second, g_meta.clone(), g.clone()).await;
    insert(&second, y_meta.clone(), y.clone()).await;
    insert(&second, x_meta.clone(), x.clone()).await;
    insert(&second, p_meta.clone(), p.clone()).await;

    let a = first.get_representation().await;
    let b = second.get_representation().await;

    // **The premise.** Everything that is a function of the *set* of blocks held is identical, so these
    // two nodes really did reach the same justifications; they differ in nothing a test could confuse
    // with content.
    assert_eq!(a.dag_set, b.dag_set, "the same blocks are held");
    assert_eq!(
        a.dag_message_state.msg_map.keys().collect::<Vec<_>>(),
        b.dag_message_state.msg_map.keys().collect::<Vec<_>>(),
        "the same messages are held"
    );
    assert_eq!(a.child_map, b.child_map, "the same parents");
    assert_eq!(a.height_map, b.height_map, "the same heights");
    assert_eq!(
        a.dag_message_state.latest_msgs, b.dag_message_state.latest_msgs,
        "and the same latest messages — H-1 admits no (sender, seq_num) tie, so this map cannot be \
         where an order difference survives"
    );

    // **And the difference.** One key, written three times, last write wins — so the two nodes disagree
    // about the state at the fringe they both hold.
    assert_eq!(
        a.fringe_states.get(&key).map(|f| f.state_hash),
        Some(Blake2b256Hash::from_byte_array(
            StateHash::new([41u8; 32]).as_bytes()
        )),
        "the last block inserted wins the key: node A ends on y's record"
    );
    assert_eq!(
        b.fringe_states.get(&key).map(|f| f.state_hash),
        Some(Blake2b256Hash::from_byte_array(
            StateHash::new([21u8; 32]).as_bytes()
        )),
        "and node B ends on p's record — the same key, a different value"
    );
    assert_ne!(
        a.fringe_states.get(&key),
        b.fringe_states.get(&key),
        "**the reproduction**: two nodes with the same blocks hold different `FringeData` at one key, \
         because that map is keyed by the fringe *set* and carries a value the set does not determine"
    );
}
