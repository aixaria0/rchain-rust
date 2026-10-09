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
//! **What this file claims, stated so it cannot be read as more.** It is a **storage-level
//! counterexample** to the premise C215's own note rests on ("delivery order alone is not a difference
//! between two nodes"), and a **candidate mechanism** for the incident — not a reproduction of TE-1
//! through block validation. The blocks here are metadata: their `fringe` and `fringe_state_hash` are
//! the *receiver's* derived values, which is what the defect is about, but they never pass through
//! `validate`, and the second test reads the rejection input one link short of `MergeScope::merge`
//! (which needs a `RhoHistoryRepository`; `casper/tests/common::build_runtime_manager` supplies one, so
//! the full-path version is buildable and is owed — see the PR discussion). A reader who wants the
//! incident reproduced end to end does not have it here.
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
    Blake2b256HashCodec, BlockHashCodec, BlockMessageCodec, BlockMetadataCodec, FringeDataCodec,
    SignedDeployDataCodec,
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
    // **The whole map, not its keys.** Comparing keys establishes that the two nodes hold the same
    // *set* of message ids and nothing about the messages behind them — and a `Message` carries the
    // sender, the sequence number, the parents, the fringe and the `seen` closure, which is most of
    // what a merge reads. Reviewing #299 caught the weaker form here; the stronger one is also the
    // simpler one.
    assert_eq!(
        a.dag_message_state.msg_map, b.dag_message_state.msg_map,
        "the same messages are held, keys *and* values"
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

// ---------------------------------------------------------------------------------------------------
// C215 at the merge's own entry point: a *validated* block set, two arrival orders, one outcome.
//
// The tests above are a storage-level counterexample — they feed metadata straight to `insert` and read
// the rejection input one link short of the merge. Review of #299 said so, correctly. This is the form
// the review asked for: two blocks per height whose post-states the **runtime computes**, a finalised
// fringe they share, and `MergeScope::merge` itself over each of two arrival orders, comparing the
// resulting state hash *and* rejected set.
// ---------------------------------------------------------------------------------------------------

mod common;

use common::fringe_state;
use rchain_block_storage::block_store::BlockStore;
use rchain_casper::block_random_seed::BlockRandomSeed;
use rchain_casper::genesis::contracts::Vault;
use rchain_casper::merging::{BlockIndex, MergeScope};
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_rholang::native_state::PosGenesis;
use rchain_rholang::system_processes::BlockData;
use rchain_rholang::util::rev_address::RevAddress;
use rchain_shared::refined::NonNegI64;

fn deploy_with(term: &str, sig: u8) -> SignedDeployData {
    SignedDeployData {
        data: DeployData {
            attachments: Vec::new(),
            term: term.to_string(),
            timestamp: 0,
            phlo_price: 1,
            phlo_limit: 500_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
        },
        deployer: vec![0u8; 65],
        sig: vec![sig],
        sig_algorithm: "secp256k1".to_string(),
    }
}

fn seeded_vault() -> Vault {
    Vault {
        rev_address: RevAddress::from_public_key(&PublicKey::new(vec![0u8; 65]))
            .expect("valid rev address"),
        initial_balance: NonNegI64::try_from(1_000_000_000).unwrap(),
    }
}

fn shell(id: u8, sender: u8, seq: i64, height: i64, parents: &[BlockHash]) -> BlockMessage {
    BlockMessage {
        version: 1,
        shard_id: "root".to_string(),
        block_hash: hash(id),
        block_number: BlockHeight::try_from(height).expect("a height"),
        sender: Validator::new([sender; 65]),
        seq_num: SeqNum::try_from(seq).expect("a sequence number"),
        pre_state_hash: StateHash::new([0u8; 32]),
        post_state_hash: StateHash::new([0u8; 32]),
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

/// Run a deploy through the runtime, returning the block with its state computed — **a block a node
/// would accept**, which is the whole point of this test.
async fn validated(
    rm: &rchain_casper::runtime_manager::RuntimeManager,
    pre: &Blake2b256Hash,
    mut b: BlockMessage,
    term: &str,
    sig: u8,
) -> (BlockMessage, Blake2b256Hash) {
    let rand = BlockRandomSeed::random_generator_from_block(&b);
    let (post, user, sys) = rm
        .compute_state(
            pre,
            &[deploy_with(term, sig)],
            &[],
            &rand,
            BlockData::from_block(&b),
            pre,
        )
        .await
        .expect("compute_state");
    b.pre_state_hash = StateHash::new(*pre.as_bytes());
    b.post_state_hash = StateHash::new(*post.as_bytes());
    b.state = RholangState {
        deploys: user.into_iter().map(|r| r.deploy).collect(),
        system_deploys: sys.into_iter().map(|r| r.deploy).collect(),
    };
    (b, post)
}

/// **The merge's own entry point, with blocks a node would accept — and a negative result.**
///
/// **This does NOT reproduce C215's divergence, and it is not the fix's falsifier.** It was written to
/// be: two arrival orders, one block set, and a comparison of `MergeOutcome::state` *and*
/// `rejected_deploys`. It passes on the unfixed tree — the two orders merge identically, with nothing
/// rejected — so the collision the storage-level test above asserts does **not** reach this merge's
/// decision in this construction, and why is not established. That matters for what may be claimed: with
/// no red-before artifact, a fix for C215 has no falsifier, and writing one on an argument would be the
/// thing this programme keeps refusing to do.
///
/// **What it is worth keeping for**: the invariant it asserts (the same validated blocks merge to the
/// same state whatever order they arrived in) is the property C215 says is violated, it is checked
/// through the real entry point with real computed states, and it would catch a regression that broke
/// it. It is a regression test and a negative result, not evidence of a defect.
///
/// **And a gap it exposed**: `MergeReport`'s `conflict_chains`/`kept_chains`/`rejected_chains` count the
/// **conflict scope only** (`conflict_set.len()`, `merging.rs:1934`), so a merge that drops a *finalised*
/// chain reports `rejected_chains: 0` and says nothing about it. `rejected_deploys` does cover it — which
/// is how the negative result above is even visible — but the report an operator reads does not.
///
/// `g` (genesis) → `x` (height 1) → `y`, `z` (height 2, concurrent siblings, different senders).
///
/// `y` and `z` both finalise the fringe `{x}` and disagree about what that fringe rejected: `y`'s block
/// records `x`'s deploy as rejected and `z`'s records nothing — which is what two concurrent proposers
/// see of each other. Their `FringeData` therefore collide on `fringe_hash_of({x})` with different
/// values, and `insert` keeps the **last one written**. Two nodes that received `y` and `z` in opposite
/// orders hold different records at that key, and `MergeScope::merge` reads it for the **final scope**
/// (`{x}`) to decide whether `x`'s chain is kept — `rejections_for` maps the record's rejected set onto
/// every block of its fringe.
///
/// So the merge over the same blocks answers differently, and this compares **the answer**: the merged
/// state hash and the rejected set. That is what review of #299 asked for and what the two tests above
/// do not do — they feed metadata straight to `insert` and stop one link short of the merge.
#[tokio::test]
async fn the_same_validated_blocks_merge_identically_in_both_arrival_orders() {
    let rm = common::build_runtime_manager().await;
    let rand = Blake2b512Random::from_init(&[0u8; 32]);
    let (genesis_pre, genesis_post, _) = rm
        .compute_genesis(
            &[],
            &rand,
            BlockData::empty(),
            &PosGenesis::default(),
            &[seeded_vault()],
        )
        .await
        .expect("compute_genesis");

    let mut g = shell(0x01, 1, 0, 0, &[]);
    g.pre_state_hash = StateHash::new(*genesis_pre.as_bytes());
    g.post_state_hash = StateHash::new(*genesis_post.as_bytes());

    let (x, x_post) = validated(
        &rm,
        &genesis_post,
        shell(0x11, 1, 1, 1, &[g.block_hash]),
        "@\"x\"!(1)",
        1,
    )
    .await;
    let (mut y, _y_post) = validated(
        &rm,
        &x_post,
        shell(0x21, 2, 0, 2, &[x.block_hash]),
        "@\"y\"!(2)",
        2,
    )
    .await;
    let (mut z, _z_post) = validated(
        &rm,
        &x_post,
        shell(0x31, 3, 0, 2, &[x.block_hash]),
        "@\"z\"!(3)",
        3,
    )
    .await;

    // The two proposers' own views of the round. `y` rejected `x`'s deploy and `z` did not; the deploy
    // id the merge matches on is the deploy's `sig` (`merging.rs:802`).
    let x_deploy_id = x.state.deploys[0].deploy.sig.clone();
    y.rejected_deploys = BTreeSet::from([x_deploy_id.clone()]);
    z.rejected_deploys = BTreeSet::new();

    // What a *receiver* records. `y` and `z` finalise the same fringe set — `{x}` — and each writes its
    // own block's rejected set into the record under that key.
    let g_meta = meta(&g, &[], 1);
    let x_meta = meta(&x, &[g.block_hash], 2);
    let y_meta = meta(&y, &[x.block_hash], 3);
    let z_meta = meta(&z, &[x.block_hash], 4);
    let key = FringeData::fringe_hash_of(&BTreeSet::from([x.block_hash]));

    // One block set, two arrival orders.
    let node_one = build_storage().await;
    for (m, b) in [
        (g_meta.clone(), g.clone()),
        (x_meta.clone(), x.clone()),
        (y_meta.clone(), y.clone()),
        (z_meta.clone(), z.clone()),
    ] {
        insert(&node_one, m, b).await;
    }
    let node_two = build_storage().await;
    for (m, b) in [
        (g_meta.clone(), g.clone()),
        (x_meta.clone(), x.clone()),
        (z_meta.clone(), z.clone()),
        (y_meta.clone(), y.clone()),
    ] {
        insert(&node_two, m, b).await;
    }

    let one = node_one.get_representation().await;
    let two = node_two.get_representation().await;

    // **The premise**: the same blocks, the same messages, the same heights.
    assert_eq!(one.dag_set, two.dag_set);
    assert_eq!(one.dag_message_state.msg_map, two.dag_message_state.msg_map);
    assert_eq!(one.child_map, two.child_map);
    assert_eq!(one.height_map, two.height_map);

    // **The difference**: one key, two values — the same collision the storage-level test asserts, now
    // with blocks whose states the runtime computed.
    assert_ne!(
        one.fringe_states.get(&key),
        two.fringe_states.get(&key),
        "the two arrival orders must leave different records at the shared key, or this test says \
         nothing about the merge below"
    );

    // The indexes the merge reads, built from the runtime's own sidecars.
    let store: BlockStore = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(BlockHashCodec),
        Arc::new(BlockMessageCodec),
    ));
    for b in [&g, &x, &y, &z] {
        store.put(&[(b.block_hash, b.clone())]).await.expect("put");
    }
    let scope = MergeScope {
        final_scope: BTreeSet::from([x.block_hash]),
        conflict_scope: BTreeSet::from([y.block_hash, z.block_hash]),
        ancestry: BTreeMap::new(),
    };
    let lookup = {
        let rm = &rm;
        let store = store.clone();
        move |h: BlockHash| {
            let store = store.clone();
            async move {
                BlockIndex::get_block_index(rm, &*build_storage().await, &store, h, fringe_state(1))
                    .await
            }
        }
    };

    let outcome_one = MergeScope::merge(
        &scope,
        x_post,
        &one.fringe_states,
        rm.get_history_repo(),
        &lookup,
        |_| 0,
    )
    .await
    .expect("the merge on node one");
    let outcome_two = MergeScope::merge(
        &scope,
        x_post,
        &two.fringe_states,
        rm.get_history_repo(),
        &lookup,
        |_| 0,
    )
    .await
    .expect("the merge on node two");

    // **The reproduction, and the fix's falsifier.** Two nodes holding the same validated blocks and
    // differing only in arrival order must merge to the same state and reject the same deploys. This
    // asserts that property; it fails on a tree where `fringe_states` keeps the last write.
    assert_eq!(
        outcome_one.state, outcome_two.state,
        "the same validated block set merged to two different states, decided by arrival order"
    );
    assert_eq!(
        outcome_one.rejected_deploys, outcome_two.rejected_deploys,
        "…and rejected different deploys, from the same set"
    );
}
