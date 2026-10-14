//! **A byte-identical re-submission of a deploy** (W-N2-05).
//!
//! The row: the term is fully parsed and normalised before the duplicate is detected, so a repeated
//! submission of an identical deploy re-does the parse work even though the pool stores it once —
//! "admitted once" as a fact about the pool and not as a fact about the *work*. The class is
//! resource-amplification: the ingress is unauthenticated, and the parser is the node's most
//! expensive non-consensus cost.
//!
//! This drives the real `multi_parent_casper::deploy` — the function that parses — against a real
//! `BlockDagKeyValueStorage`, so the admission, parse and commit order is the node's own. The reading
//! is the **second** submission's answer: a refusal naming the duplicate is the pool answering before
//! the parse, and anything else is the parse running again.

use std::sync::Arc;

use rchain_block_storage::dag::codecs::{
    BlockHashCodec, BlockMetadataCodec, SignedDeployDataCodec,
};
use rchain_block_storage::dag::dag_storage::{BlockDagStorage, DeployId};
use rchain_casper::block_metadata_store::BlockMetadataStore;
use rchain_casper::dag::BlockDagKeyValueStorage;
use rchain_models::block_hash::BlockHash;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_shared::store::{InMemoryKeyValueStore, KeyValueStore};
use rchain_shared::typed_store::{BytesCodec, KeyValueTypedStore, KeyValueTypedStoreCodec};

type Shared = Arc<tokio::sync::Mutex<Box<dyn KeyValueStore + Send + Sync>>>;

fn in_memory() -> Shared {
    Arc::new(tokio::sync::Mutex::new(Box::new(
        InMemoryKeyValueStore::default(),
    )))
}

#[tokio::test]
async fn a_byte_identical_resubmission_is_parsed_and_admitted_again() {
    let metadata_store = Arc::new(
        BlockMetadataStore::create(Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(BlockHashCodec),
            Arc::new(BlockMetadataCodec),
        )))
        .await
        .expect("an in-memory metadata store"),
    );
    let deploy_index: Arc<dyn KeyValueTypedStore<DeployId, BlockHash>> = Arc::new(
        KeyValueTypedStoreCodec::new(in_memory(), Arc::new(BytesCodec), Arc::new(BlockHashCodec)),
    );
    let deploy_store: Arc<dyn KeyValueTypedStore<DeployId, SignedDeployData>> =
        Arc::new(KeyValueTypedStoreCodec::new(
            in_memory(),
            Arc::new(BytesCodec),
            Arc::new(SignedDeployDataCodec),
        ));
    let storage = BlockDagKeyValueStorage::create(metadata_store, deploy_index, deploy_store)
        .await
        .expect("the DAG store");

    let deploy = SignedDeployData {
        data: DeployData {
            attachments: Vec::new(),
            term: "Nil".to_string(),
            timestamp: 0,
            phlo_price: 1,
            phlo_limit: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
        },
        deployer: vec![0u8; 65],
        sig: vec![7u8; 65],
        sig_algorithm: "secp256k1".to_string(),
    };

    rchain_casper::multi_parent_casper::deploy(&storage, &deploy)
        .await
        .expect("the first submission parses and is pooled");

    // **The reading, and the arm asserts the deviation rather than hoping against it.** The identical
    // bytes again: `sig` is the pool's key, so the node already holds this deploy — and the term is
    // parsed and normalised a second time anyway, because nothing asks the pool before the parse.
    let second = rchain_casper::multi_parent_casper::deploy(&storage, &deploy).await;
    assert!(
        second.is_ok(),
        "**the second submission is admitted** (W-N2-05): a byte-identical deploy re-enters the \
         parse-and-commit path. Was: {second:?}"
    );

    // **And it is one copy.** That contrast is the row exactly — "admitted once" is true of the pool
    // and false of the work: the pool keys on `sig`, so the second admission overwrites nothing, and
    // the parse it paid for was spend on a deploy the node already had. If this assertion ever fails
    // while the one above passes, the reading is different and worth saying differently: the pool
    // would be accumulating copies.
    let pooled = storage
        .pooled_deploys()
        .await
        .expect("the pool is readable");
    assert_eq!(
        pooled.len(),
        1,
        "the pool stores the deploy once — so the second parse bought the node nothing at all"
    );
}
