//! **Arm B of #139's campaign: the LFS restore shape gives every restored block no fringe, so the
//! node reads a zero fringe state where the proposer read a real one.**
//!
//! Arm A (`determinism.rs`) proved the consequence: the epoch seed is anchored to the fringe state,
//! so a node that derives a *different* fringe replays the same block to a different post-state and
//! `handle_errors` reports `InvalidStateHash`. This arm proves the **input difference** — and it is
//! the decisive one, because it is the only arm whose outcome maps to a fix in this tree.
//!
//! **The claim, at the source.** `populate_dag` (`casper/src/engine/node_syncing.rs`) inserts every
//! **non-genesis** block a node restores as `BlockMetadata::from_block`, and the real `fringe` and
//! `fringe_state_hash` are a *local* recomputation produced by validation — they are not on the wire,
//! so `from_block` leaves them empty and zero. `BlockDagKeyValueStorage::insert` then caches
//! `fringe_states[fringe_hash_of(∅)] → state_hash 0`, and `get_pre_state_for_parents` begins from an
//! empty fringe and reads that zero.
//!
//! **Why it has never been seen.** A joiner syncing at *genesis* restores block 0 alone, and the
//! genesis is inserted by `insert_genesis` with the correct fringe. This needs a joiner syncing a
//! chain that is already **mature** — which is what `tools/devnet.sh reset <node>` stages for Arm D'.
//!
//! The test is written against the **real** `BlockDagKeyValueStorage` and the **real** helpers
//! `populate_dag` uses, so it observes the storage the node would have rather than a model of it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rchain_block_storage::dag::codecs::{
    Blake2b256HashCodec, BlockHashCodec, BlockMetadataCodec, FringeDataCodec, SignedDeployDataCodec,
};
use rchain_block_storage::dag::dag_storage::BlockDagStorage;
use rchain_block_storage::dag::message_map;
use rchain_casper::block_metadata_store::BlockMetadataStore;
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_models::block_hash::BlockHash;
use rchain_models::block_metadata::BlockMetadata;
use rchain_models::casper::protocol::casper_message::{
    BlockMessage, RholangState, SignedDeployData,
};
use rchain_models::fringe_data::FringeData;
use rchain_models::validator::Validator;
use rchain_shared::refined::{BlockHeight, SeqNum};
use rchain_shared::store::{InMemoryKeyValueStore, KeyValueStore};
use rchain_shared::typed_store::{BytesCodec, KeyValueTypedStoreCodec};

type Shared = Arc<tokio::sync::Mutex<Box<dyn KeyValueStore + Send + Sync>>>;

fn in_memory() -> Shared {
    Arc::new(tokio::sync::Mutex::new(Box::new(
        InMemoryKeyValueStore::default(),
    )))
}

/// The storage the node builds, constructed the way `casper/src/dag.rs`'s own tests construct it — the
/// same four stores, so `fringe_states` is populated by the production `insert` rather than by hand.
async fn build_storage() -> Arc<rchain_casper::dag::BlockDagKeyValueStorage> {
    let metadata_store = Arc::new(
        BlockMetadataStore::create(Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(BlockHashCodec),
            Arc::new(BlockMetadataCodec),
        )))
        .await
        .expect("metadata store"),
    );
    let fringe_store: Arc<
        dyn rchain_shared::typed_store::KeyValueTypedStore<Blake2b256Hash, FringeData>,
    > = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(Blake2b256HashCodec),
        Arc::new(FringeDataCodec),
    ));
    let deploy_index: Arc<
        dyn rchain_shared::typed_store::KeyValueTypedStore<
            rchain_block_storage::dag::dag_storage::DeployId,
            BlockHash,
        >,
    > = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(BytesCodec),
        Arc::new(BlockHashCodec),
    ));
    let deploy_store: Arc<
        dyn rchain_shared::typed_store::KeyValueTypedStore<
            rchain_block_storage::dag::dag_storage::DeployId,
            SignedDeployData,
        >,
    > = Arc::new(KeyValueTypedStoreCodec::new(
        in_memory(),
        Arc::new(BytesCodec),
        Arc::new(SignedDeployDataCodec),
    ));
    Arc::new(
        rchain_casper::dag::BlockDagKeyValueStorage::create(
            metadata_store,
            fringe_store,
            deploy_index,
            deploy_store,
        )
        .await
        .expect("dag storage"),
    )
}

fn hash(byte: u8) -> BlockHash {
    BlockHash::new([byte; 32])
}

fn validator(byte: u8) -> Validator {
    Validator::new([byte; 65])
}

/// A block at `height`, justifying `parent`. Its state hashes are placeholders: what this arm reads is
/// the *metadata* the DAG derives, and the two ways of deriving it are the whole subject.
fn block(h: BlockHash, height: i64, sender: Validator, parent: Option<BlockHash>) -> BlockMessage {
    BlockMessage {
        version: 1,
        shard_id: "root".to_string(),
        block_hash: h,
        block_number: BlockHeight::try_from(height).expect("height"),
        sender,
        seq_num: SeqNum::zero(),
        pre_state_hash: rchain_models::block::state_hash::StateHash::new([byte_of(height); 32]),
        post_state_hash: rchain_models::block::state_hash::StateHash::new(
            [byte_of(height) + 1; 32],
        ),
        justifications: parent.into_iter().collect(),
        bonds: BTreeMap::new(),
        rejected_deploys: BTreeSet::new(),
        rejected_blocks: BTreeSet::new(),
        rejected_senders: BTreeSet::new(),
        state: RholangState::default(),
        sig_algorithm: "secp256k1".to_string(),
        sig: vec![1],
        timestamp: 0,
    }
}

fn byte_of(height: i64) -> u8 {
    u8::try_from(height).unwrap_or(0).wrapping_add(1)
}

/// **The restore shape: a block inserted as `populate_dag` inserts it carries no fringe, and the DAG
/// caches its state under the *empty* fringe.**
///
/// This is the input difference #139 is about, observed on the real storage:
///
/// 1. `BlockMetadata::from_block` — the constructor `populate_dag` uses — leaves `fringe` empty and
///    `fringe_state_hash` zero, because those are a local recomputation and not block fields.
/// 2. `insert` therefore writes `fringe_states[fringe_hash_of(∅)] = { state_hash: 0 }` — the same key
///    for **every** restored block, so they overwrite each other and the chain's real fringe key is
///    never written at all.
/// 3. The message map carries that empty fringe into every `Message`, and `latest_fringe` reads it —
///    so a parent set made of restored blocks names the empty fringe.
///
/// **Observed red** by giving `from_block` a fringe (or by inserting with validation-derived
/// metadata): the lookups below then find a real key and the assertions fail. The contrast arm at the
/// end is what makes that concrete rather than asserted.
#[tokio::test]
async fn a_restored_block_carries_no_fringe_and_its_state_is_cached_under_the_empty_fringe() {
    let dag = build_storage().await;

    let genesis = block(hash(1), 0, validator(1), None);
    let child = block(hash(2), 1, validator(2), Some(hash(1)));

    // **Exactly what `populate_dag` does**, in its order: genesis through `insert_genesis` (which
    // gives it the correct fringe), then every later block through `from_block`.
    rchain_block_storage::syntax::insert_genesis(&*dag, genesis.clone())
        .await
        .expect("the restored genesis inserts");
    let restored_metadata = BlockMetadata::from_block(&child);
    dag.insert(restored_metadata.clone(), child.clone())
        .await
        .expect("a restored block inserts");

    // 1. The metadata the restore path derives has no fringe, and a zero fringe state.
    assert!(
        restored_metadata.fringe.is_empty(),
        "the restore shape: `BlockMetadata::from_block` cannot know a block's fringe, because the \
         fringe is not a block field"
    );
    assert_eq!(
        restored_metadata.fringe_state_hash,
        rchain_models::block::state_hash::StateHash::new([0u8; 32]),
        "and its fringe state hash is zero, not the state the proposer anchored"
    );

    // 2. The DAG's cache is therefore keyed by the *empty* fringe.
    let empty_key = FringeData::fringe_hash_of(&BTreeSet::new());
    let repr = dag.get_representation().await;
    let record = repr
        .fringe_states
        .get(&empty_key)
        .expect("the empty fringe is the key the restored block wrote under");
    assert_eq!(
        *record.state_hash.as_bytes(),
        [0u8; 32],
        "and the state behind it is the zero hash — this is the value the replay would anchor the \
         next epoch's seed to"
    );

    // 3. The message map carries the empty fringe onward, so the *parent set* names it too.
    let message = rchain_casper::dag::message_from_block_metadata(
        &restored_metadata,
        &repr.dag_message_state.msg_map,
    )
    .expect("the restored block's justification is in the message map");
    assert!(
        message.fringe.is_empty(),
        "a message from a restored metadata carries an empty fringe, which is what `latest_fringe` \
         reads: the parent set a validator derives names the empty fringe, not the chain's real one"
    );
    let parents: BTreeSet<_> = [message].into_iter().collect();
    let derived_fringe = message_map::latest_fringe(&repr.dag_message_state.msg_map, &parents);
    assert!(
        derived_fringe.is_empty(),
        "so `latest_fringe` over restored parents returns the empty set — and the lookup that \
         follows it is the one above, whose value is zero"
    );
}

/// **The contrast, and the reason the test above is not vacuous.**
///
/// The same block, inserted with the metadata a *validating* node derives — a real fringe and the
/// state the merge produced — lands under a **different** cache key with a **non-zero** state. So the
/// two paths really do disagree about the same block, which is the `Split` shape: two readers of one
/// block answering different questions.
///
/// Without this arm, "the restored metadata has no fringe" could be a fact about `from_block` that
/// nothing ever consults.
#[tokio::test]
async fn the_two_paths_cache_the_same_block_under_different_fringe_keys() {
    let dag = build_storage().await;

    let genesis = block(hash(1), 0, validator(1), None);
    let genesis_meta = BlockMetadata::from_block(&genesis);
    dag.insert(genesis_meta, genesis.clone())
        .await
        .expect("genesis inserts");

    let child = block(hash(2), 1, validator(2), Some(hash(1)));

    // The restore path: no fringe.
    let restored_fringe_key = FringeData::fringe_hash_of(&BlockMetadata::from_block(&child).fringe);
    // The validating path: the merge's own answer. A real fringe set and a real state.
    let validating_fringe: BTreeSet<BlockHash> = [hash(1)].into_iter().collect();
    let validating_fringe_key = FringeData::fringe_hash_of(&validating_fringe);
    let validating_state = rchain_models::block::state_hash::StateHash::new([0x9a; 32]);
    let validating_metadata = BlockMetadata {
        fringe: validating_fringe,
        fringe_state_hash: validating_state,
        ..BlockMetadata::from_block(&child)
    };

    dag.insert(validating_metadata, child.clone())
        .await
        .expect("a validated block inserts");

    assert_ne!(
        restored_fringe_key, validating_fringe_key,
        "the two paths must disagree about which key the block's fringe belongs under — that \
         disagreement is the defect"
    );
    assert_eq!(
        restored_fringe_key,
        FringeData::fringe_hash_of(&BTreeSet::new()),
        "the restore path's key is the empty fringe"
    );
    let repr = dag.get_representation().await;
    assert_eq!(
        repr.fringe_states
            .get(&validating_fringe_key)
            .expect("the validating path wrote its own key")
            .state_hash
            .as_bytes()
            .clone(),
        [0x9a; 32],
        "and behind it is the state the merge produced, not zero"
    );
}
