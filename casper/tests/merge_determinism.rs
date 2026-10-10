//! **C215**: does a node's answer depend on the order it received the same blocks in? Two DAGs, one
//! block set, two arrival orders — and `fringe_states` is where the row's own note said no difference
//! could exist.
//!
//! The note (`spec/findings.tsv`, C215) reasons that "delivery order alone is not a difference between
//! two nodes — only different *content* is", because `child_map`, `msg_map` and the height map are
//! `BTreeMap`/`BTreeSet` over the same set, and `latest_msgs` is order-sensitive only on a tie, which the
//! H-1 gate refuses before any write. That reasoning is right about those four, and it misses a fifth map
//! in the merge's input: **`fringe_states`**.
//!
//! `FringeData` is keyed by `fringe_hash_of(fringe_set)` and carries **per-block** values —
//! `state_hash`, `rejected_deploys`, `rejected_blocks`, `rejected_senders` (`models/src/fringe_data.rs`)
//! — while its `Hash` impl hashes only the key, i.e. the type's own identity *is* the key. Two blocks
//! that finalise **the same fringe set** but carry different reports are therefore the *same key* with
//! two values, and `insert` used to resolve that last-write-wins: two nodes holding the same blocks in
//! different orders held different `fringe_states[K]`, and read it unconditionally — for the merged base
//! state (`multi_parent_casper.rs::get_pre_state_for_parents`) and for the rejection sets
//! (`merging.rs::rejections_for`).
//!
//! **The fix, and what it is falsified by.** `insert` now *joins* a record already stored at a key with
//! the incoming one instead of overwriting it — the rejection sets and `fringe_diff` by union (monotone,
//! so the merges reading them can only reject more), `state_hash` by a deterministic tie-break, since
//! hashes cannot be joined (see `casper/src/dag.rs::join_fringe_records`). The join is commutative,
//! associative and idempotent, so every arrival order reaches one record. Two tests are the falsifier and
//! are **red before it, green after**:
//!
//! 1. [`two_arrival_orders_of_one_block_set_leave_one_fringe_cache`] — the storage, through the
//!    production `insert`.
//! 2. [`the_same_validated_blocks_merge_identically_in_both_arrival_orders`] — the merge's own entry
//!    point, with block states the runtime computes.
//!
//! **What is left open, stated so it is not read as more.** The collision needs two blocks that
//! *disagree about one fringe* — genuinely different content. Whether two honest nodes holding the same
//! block set can ever produce such a pair is not settled here; the argument that they cannot (the merge
//! for a fringe is a function of the DAG, so same-DAG nodes compute the same report) is reason to expect
//! a divergence to be propagated rather than created at this level. What the join restores holds either
//! way: the value at a fringe key is a function of that key.

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

/// The same, with a state the **runtime** produced rather than a byte pattern — for a record whose
/// value has to be readable (`compute_bonds` runs against it) rather than merely distinct.
fn meta_state(
    b: &BlockMessage,
    fringe: &BTreeSet<BlockHash>,
    state: &Blake2b256Hash,
) -> BlockMetadata {
    let mut m = BlockMetadata::from_block(b);
    m.fringe = fringe.clone();
    m.fringe_state_hash = StateHash::new(*state.as_bytes());
    m
}

async fn insert(dag: &BlockDagKeyValueStorage, m: BlockMetadata, b: BlockMessage) {
    dag.insert(m, b).await.expect("the block inserts");
}

/// **Two arrival orders, one block set, one fringe cache.**
///
/// `p`, `x` and `y` all justify the genesis block `g` and all carry `fringe = {g}` — the same fringe
/// *set*, so the same key `K` — while disagreeing about that set's state: `p`, `x` and `y` each claim a
/// different `fringe_state_hash`. They are three blocks at one height by three different senders, which is
/// ordinary multi-proposer behaviour, not an offence.
///
/// The two nodes insert the same four blocks in opposite orders. The assertions are ordered so the test
/// cannot pass vacuously: the four maps that *are* functions of the block set must be **equal** (that is
/// the premise — "the same justifications"), and the fifth, `fringe_states`, must be equal **too** —
/// because a record at a fringe key is a function of that key. Before the join this last assertion was
/// three assertions to the opposite effect (`state_hash` 41 on one node, 21 on the other), which is what
/// made the merge-path test below answer differently per order.
#[tokio::test]
async fn two_arrival_orders_of_one_block_set_leave_one_fringe_cache() {
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

    // **And the record is the same.** One key, written three times, and the value is a function of the
    // key: `insert` joins rather than overwrites, so the arrival order cannot show through. Before the
    // join this was three assertions to the opposite effect — node A ended on `y`'s record and node B on
    // `p`'s, `state_hash` 41 against 21 — and that difference is what the merge-path test below turns
    // into two different outcomes.
    assert_eq!(
        a.fringe_states.get(&key),
        b.fringe_states.get(&key),
        "**the fix**: two nodes with the same blocks hold the *same* `FringeData` at one key — a record \
         is a function of its fringe, which is `FringeData`'s own identity"
    );
    assert_eq!(
        a.fringe_states.get(&key).map(|f| f.state_hash),
        Some(Blake2b256Hash::from_byte_array(
            StateHash::new([21u8; 32]).as_bytes()
        )),
        "the three writers report states 21, 31 and 41 for one fringe; they cannot be joined, so the \
         smaller is kept — the same value on both nodes"
    );
    assert!(
        a.fringe_states
            .get(&key)
            .is_some_and(|f| f.rejected_deploys.is_empty()),
        "and the rejection sets — empty here on all three — are unioned, not replaced"
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
use rchain_casper::multi_parent_casper::get_pre_state_for_parents;
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

/// **The fix's falsifier: the merge's own entry point, with blocks a node would accept.**
///
/// Two arrival orders, one block set, and a comparison of `MergeOutcome::state` *and*
/// `rejected_deploys`. **Red before the join in `casper/src/dag.rs::insert`, green after** — the
/// earlier version of this test passed on the unfixed tree only because its collision did not reach
/// the merge's decision: the final-scope chain and the conflict-scope chains were unrelated, so
/// flipping `accepted_finally`/`rejected_finally` changed nothing. `incompatible_with_final` reads
/// `conflicts_map[x]` when `x` is accepted-finally and `dependency_map[x]` when it is rejected, so the
/// falsifier needs a chain that **depends on `x` and does not conflict with it** — which is why `y`
/// consumes the event `x` produces. With that, the two orders put `y` in `to_merge` on one node and in
/// `rejected` on the other: two states, two rejected sets.
///
/// `g` (genesis) → `x` (height 1) → `y`, `z` (height 2, concurrent siblings, different senders).
///
/// `y` and `z` both finalise the fringe `{x}` and report that fringe differently: `y`'s block records
/// `x`'s deploy as rejected and `z`'s records nothing. Their `FringeData` are therefore the **same
/// key** (`fringe_hash_of({x})`) with different values. `insert` used to keep the last one written, so
/// two nodes that received `y` and `z` in opposite orders held different records at that key;
/// `MergeScope::merge` reads it for the **final scope** (`{x}`) to decide whether `x`'s chain is kept
/// (`rejections_for` maps the record's rejected set onto every block of its fringe), and from there the
/// answer differed. With the join, both orders reach one record and one answer.
///
/// **What the construction's reachability leaves open.** The pair is only producible by two blocks that
/// disagree about one fringe — genuinely different content. Whether two honest nodes holding the *same*
/// block set can ever produce such a pair is not settled here; the argument that they cannot (the merge
/// for a fringe is a function of the DAG, so same-DAG nodes compute the same report) is a reason to
/// expect the divergence to be propagated rather than created at this level. The invariant the join
/// restores — a record is a function of its key, which is `FringeData`'s own identity — holds either
/// way, and this test is its falsifier.
///
/// **A gap it exposed**: `MergeReport`'s `conflict_chains`/`kept_chains`/`rejected_chains` count the
/// **conflict scope only** (`conflict_set.len()`, `merging.rs:1934`), so a merge that drops a *finalised*
/// chain reports `rejected_chains: 0` and says nothing about it. `rejected_deploys` does cover it, but
/// the report an operator reads does not (AUDIT C260).
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
        "@\"c\"!(1)",
        1,
    )
    .await;
    let (mut y, _y_post) = validated(
        &rm,
        &x_post,
        shell(0x21, 2, 0, 2, &[x.block_hash]),
        "for(_ <- @\"c\"){@\"y\"!(2)}",
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

    // **The collision, and what the fix does with it.** `y` and `z` finalise the same fringe set and
    // carry different reports, so the key is the same and the values are not: this is the collision
    // the storage-level test asserts, now with blocks whose states the runtime computed. Before the
    // fix the two orders left *different* records here (this assertion was `assert_ne!`, and it held),
    // and the merge below therefore answered differently. With the join in `insert` both orders reach
    // one record — which is what the merge's two assertions at the end then read.
    assert_eq!(
        one.fringe_states.get(&key),
        two.fringe_states.get(&key),
        "the record at a fringe key is a function of that key, so the two arrival orders must leave \
         the same one — if they do not, `insert` is still last-write-wins and the merge below will \
         diverge"
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
    // asserts that property; it fails on a tree where the record at a fringe key keeps the last write.
    assert_eq!(
        outcome_one.state, outcome_two.state,
        "the same validated block set merged to two different states, decided by arrival order"
    );
    assert_eq!(
        outcome_one.rejected_deploys, outcome_two.rejected_deploys,
        "…and rejected different deploys, from the same set"
    );
}

// ---------------------------------------------------------------------------------------------------
// C250: is the colliding pair those tests build reachable from two honest siblings? It is not — the
// value is the parent set's, and a block's own deploys are not an input to it.
// ---------------------------------------------------------------------------------------------------

/// **C250's answer, pinned: two honest siblings contribute one value to the fringe they share.**
///
/// The tests above construct the collision by *assigning* it — `y.rejected_deploys = {x's deploy}` and
/// `z.rejected_deploys = {}` — which is the hypothesis the row is about rather than an observation of
/// it, and review of #299 read the construction as "ordinary multi-proposer behaviour". This takes the
/// same pair of siblings and asks the **production derivation** what each of them would write.
///
/// There is nothing for them to differ on. `get_pre_state_for_parents(dag, block_store, runtime,
/// parent_hashes, block_index)` has **no argument for the block being built**: a proposer takes its
/// `rejected_deploys` from that call's `fringe_rejected_deploys` — the **fringe** merge's rejections
/// (`blocks/proposer/block_creator.rs:90`) — and its `fringe` and `fringe_state` from the same call. Two
/// siblings share `parent_hashes`, so all three are one value per parent set.
///
/// What a sibling's own conflict scope resolves is a **different field**: here, two consumers racing for
/// the single produce `x` leaves on `@"c"`. That lands in `ParentsMergedState::rejected_deploys`, whose
/// only consumer in the tree is a log line (`blocks/proposer/proposer.rs:890`) — it is never written to a
/// block and never reaches a record.
///
/// **The control is half the test.** Two identical calls returning the same value proves little, so the
/// second DAG below changes one record and shows the derivation move: the same blocks, with `x` claiming
/// a different fringe, derive a different `prev_fringe` — the base every merge for that block starts
/// from. The equality above is therefore a fact about two honest siblings over one DAG, not about a
/// derivation that returns a constant.
///
/// **What this does not settle, stated so it is not read as more.** It fixes the siblings' parent set,
/// and `prev_fringe` with it — and `prev_fringe` is the **base** every merge for that block starts from,
/// while the record's key is `fringe_hash_of(fringe)` alone. The key therefore does not name the base: a
/// pair of *non*-sibling blocks that reach one fringe from two different bases would still write two
/// different values at one key, and nothing here excludes that. The derivation's sensitivity to the DAG
/// (the control) is what would make such a pair differ, which is why the control is the other half of
/// this test rather than an aside. Register row C250 carries the residue.
///
/// **The rig's own limit.** The fringe does not advance past genesis here, so the shared key is the
/// genesis fringe's rather than one the walk produced. What the test exercises is therefore the
/// derivation's *inputs* — absent, for a block — rather than an advancing walk; the shape where the walk
/// does advance is the one the tests above build by hand (`meta(&y, &[x.block_hash], 3)`).
#[tokio::test]
async fn two_honest_siblings_contribute_one_value_to_the_fringe_they_share() {
    let rm = common::build_runtime_manager().await;
    let rand = Blake2b512Random::from_init(&[0u8; 32]);
    // A plain genesis. The fringe does **not** advance past it in this rig — a single producer's chain
    // finalises on a *later* justification's seeing of its block, and these synthetic blocks are not
    // that — so the key the siblings share below is the genesis fringe's. That does not weaken the test:
    // the claim is about what a block *contributes* to the derivation, and `parent_hashes` is the whole
    // of it. The control at the end shows the derivation is not returning a constant.
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
        "@\"c\"!(1)",
        1,
    )
    .await;

    // Two siblings that genuinely contend: both consume the single produce `x` leaves on `@"c"`. Each is
    // a block a node would accept on its own. They are not alternatives one proposer weighs — they are
    // two proposers' blocks for one round, which is the shape the row is about.
    let (y, _y_post) = validated(
        &rm,
        &x_post,
        shell(0x21, 2, 0, 2, &[x.block_hash]),
        "for(_ <- @\"c\"){@\"y\"!(1)}",
        2,
    )
    .await;
    let (z, _z_post) = validated(
        &rm,
        &x_post,
        shell(0x31, 3, 0, 2, &[x.block_hash]),
        "for(_ <- @\"c\"){@\"z\"!(1)}",
        3,
    )
    .await;
    assert_ne!(
        y.post_state_hash, z.post_state_hash,
        "the two siblings are different content — one parent set, different deploys, different state \
         — so a value that comes out equal did not come out of their content"
    );
    assert!(
        !y.state.deploys.is_empty() && !z.state.deploys.is_empty(),
        "and each really carries its deploy, so neither is vacuous"
    );
    // **The input, named.** `parent_hashes` is the only thing a block contributes to the derivation, and
    // two proposers for one round justify the same set — which is the whole of the argument.
    assert_eq!(
        y.justifications, z.justifications,
        "the siblings justify one parent set; that set is the derivation's *only* input from a block"
    );

    // The proposer's DAG while it builds *either* sibling: g and x, and neither sibling yet.
    let proposer = build_storage().await;
    insert(
        &proposer,
        meta_state(&g, &BTreeSet::new(), &genesis_post),
        g.clone(),
    )
    .await;
    insert(
        &proposer,
        meta_state(&x, &BTreeSet::from([g.block_hash]), &genesis_post),
        x.clone(),
    )
    .await;

    let store: BlockStore = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(BlockHashCodec),
        Arc::new(BlockMessageCodec),
    ));
    for b in [&g, &x] {
        store.put(&[(b.block_hash, b.clone())]).await.expect("put");
    }
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

    let parents = BTreeSet::from([x.block_hash]);
    let pre_y = get_pre_state_for_parents(&*proposer, &store, &rm, &parents, &lookup)
        .await
        .expect("the derivation a proposer of y runs");
    let pre_z = get_pre_state_for_parents(&*proposer, &store, &rm, &parents, &lookup)
        .await
        .expect("the derivation a proposer of z runs");

    // **The answer.** Nothing about which sibling is being built reaches the derivation.
    assert_eq!(pre_y.fringe, pre_z.fringe, "one parent set, one fringe");
    assert_eq!(
        pre_y.fringe_state, pre_z.fringe_state,
        "and one state at that fringe"
    );
    assert_eq!(
        pre_y.fringe_rejected_deploys, pre_z.fringe_rejected_deploys,
        "and one set of rejections — the fringe merge's, which is what `block_creator.rs:90` puts on \
         the block; the siblings' own racing consumes are a *different* field and never reach it"
    );

    // So both write the same value under the same key, in either arrival order.
    let key = FringeData::fringe_hash_of(&pre_y.fringe);
    let g_meta = meta_state(&g, &BTreeSet::new(), &genesis_post);
    let x_meta = meta_state(&x, &BTreeSet::from([g.block_hash]), &genesis_post);
    let y_meta = meta_state(&y, &pre_y.fringe, &pre_y.fringe_state);
    let z_meta = meta_state(&z, &pre_z.fringe, &pre_z.fringe_state);
    assert_eq!(
        FringeData::fringe_hash_of(&y_meta.fringe),
        key,
        "both siblings' records land on the key the derivation named"
    );

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
    assert_eq!(
        one.fringe_states.get(&key),
        two.fringe_states.get(&key),
        "two arrival orders of two honest siblings leave one record — not because the join merged two \
         claims, but because there was only ever one"
    );
    assert_eq!(
        one.fringe_states.get(&key).map(|f| f.state_hash),
        Some(Blake2b256Hash::from_byte_array(
            pre_y.fringe_state.as_bytes()
        )),
        "and it is the derivation's value, not a tie-break between two"
    );

    // **The control.** The same blocks, with one record changed: `x` now claims no fringe, so the
    // derivation starts from a different base. If this were equal to the above, the equality would be a
    // fact about a constant rather than about two honest siblings.
    let moved = build_storage().await;
    insert(
        &moved,
        meta_state(&g, &BTreeSet::new(), &genesis_post),
        g.clone(),
    )
    .await;
    insert(
        &moved,
        meta_state(&x, &BTreeSet::new(), &genesis_post),
        x.clone(),
    )
    .await;
    let pre_moved = get_pre_state_for_parents(&*moved, &store, &rm, &parents, &lookup)
        .await
        .expect("the derivation over the changed DAG");
    assert_ne!(
        pre_moved.prev_fringe, pre_y.prev_fringe,
        "the derivation reads the DAG: a different `x` record is a different base, which is what makes \
         the equalities above evidence rather than arithmetic"
    );
}
