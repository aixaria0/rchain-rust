//! Block metadata store — the in-memory DAG index over the persisted metadata store (port of
//! `block-storage/dag/BlockMetadataStore.scala`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rchain_block_storage::dag::metadata_store::{
    add_block_to_dag_state_mut, block_metadata_to_info, recreate_in_memory_state,
    validate_dag_state, BlockInfo, DagState,
};
use rchain_models::block_hash::BlockHash;
use rchain_models::block_metadata::BlockMetadata;
use rchain_shared::refined::BlockHeight;
use rchain_shared::typed_store::KeyValueTypedStore;

/// The block metadata store: a persisted `KeyValueTypedStore` plus an in-memory [`DagState`] index
/// rebuilt on startup.
pub struct BlockMetadataStore {
    store: Arc<dyn KeyValueTypedStore<BlockHash, BlockMetadata>>,
    dag_state: tokio::sync::RwLock<DagState>,
}

impl BlockMetadataStore {
    /// Rebuild the in-memory DAG index from the persisted store (port of `BlockMetadataStore.apply`).
    pub async fn create(
        store: Arc<dyn KeyValueTypedStore<BlockHash, BlockMetadata>>,
    ) -> Result<Self, String> {
        let blocks = store.to_map().await?;
        let info_map: BTreeMap<BlockHash, BlockInfo> = blocks
            .iter()
            .map(|(hash, meta)| (*hash, block_metadata_to_info(meta)))
            .collect();
        let dag_state = recreate_in_memory_state(&info_map)?;
        Ok(BlockMetadataStore {
            store,
            dag_state: tokio::sync::RwLock::new(dag_state),
        })
    }

    /// Insert a block's metadata into both the in-memory index and the persisted store.
    pub async fn add(&self, block: BlockMetadata) -> Result<(), String> {
        let info = block_metadata_to_info(&block);
        {
            let mut state = self.dag_state.write().await;
            // In place: the index is `Arc`-shared with the DAG representation, so this copies a map
            // only if a reader still holds the previous one (AUDIT C56's owed paragraph).
            add_block_to_dag_state_mut(&info, &mut state);
            validate_dag_state(&state)?;
        }
        self.store.put(&[(block.block_hash, block)]).await?;
        Ok(())
    }

    pub async fn get(&self, hash: &BlockHash) -> Result<Option<BlockMetadata>, String> {
        let vals = self.store.get(&[*hash]).await?;
        Ok(vals.into_iter().next().flatten())
    }

    /// Look up a block's metadata, failing if it is absent (port of `getUnsafe`).
    pub async fn get_unchecked(&self, hash: &BlockHash) -> Result<BlockMetadata, String> {
        self.get(hash)
            .await?
            .ok_or_else(|| format!("BlockMetadataStore is missing key {}", hash.to_hex()))
    }

    pub async fn contains(&self, hash: &BlockHash) -> bool {
        self.dag_state.read().await.dag_set.contains(hash)
    }

    /// The index's own allocation, not a copy of it: the caller shares the map the store updates
    /// (AUDIT C56's owed paragraph — this used to hand out a full clone per insert).
    pub async fn dag_set(&self) -> Arc<BTreeSet<BlockHash>> {
        self.dag_state.read().await.dag_set.clone()
    }

    pub async fn child_map_data(&self) -> Arc<BTreeMap<BlockHash, BTreeSet<BlockHash>>> {
        self.dag_state.read().await.child_map.clone()
    }

    pub async fn height_map(&self) -> Arc<BTreeMap<BlockHeight, BTreeSet<BlockHash>>> {
        self.dag_state.read().await.height_map.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_block_storage::dag::codecs::{BlockHashCodec, BlockMetadataCodec};
    use rchain_shared::store::{InMemoryKeyValueStore, KeyValueStore};
    use rchain_shared::typed_store::KeyValueTypedStoreCodec;

    type Shared = Arc<tokio::sync::Mutex<Box<dyn KeyValueStore + Send + Sync>>>;

    fn metadata_store() -> Arc<dyn KeyValueTypedStore<BlockHash, BlockMetadata>> {
        let shared: Shared = Arc::new(tokio::sync::Mutex::new(Box::new(
            InMemoryKeyValueStore::default(),
        )));
        Arc::new(KeyValueTypedStoreCodec::new(
            shared,
            Arc::new(BlockHashCodec),
            Arc::new(BlockMetadataCodec),
        ))
    }

    fn hash(byte: u8) -> BlockHash {
        let mut bytes = [0u8; 32];
        bytes[0] = byte;
        BlockHash::new(bytes)
    }

    fn meta(hash: BlockHash, parents: &[BlockHash], block_num: i64) -> BlockMetadata {
        BlockMetadata {
            block_hash: hash,
            block_num: rchain_shared::refined::BlockHeight::try_from(block_num).unwrap(),
            sender: rchain_models::validator::Validator::new([0u8; 65]),
            seq_num: 0.try_into().unwrap(),
            justifications: parents.iter().copied().collect(),
            bonds_map: BTreeMap::new(),
            validated: true,
            validation_failed: false,
            slashable: false,
            fringe: BTreeSet::new(),
            fringe_state_hash: rchain_models::block::state_hash::StateHash::new([0u8; 32]),
            member_of_fringe: None,
        }
    }

    #[tokio::test]
    async fn add_and_lookup_round_trip() {
        let store = BlockMetadataStore::create(metadata_store()).await.unwrap();
        let genesis = meta(hash(0), &[], 0);
        store.add(genesis.clone()).await.unwrap();

        assert!(store.contains(&hash(0)).await);
        assert_eq!(store.get(&hash(0)).await.unwrap(), Some(genesis.clone()));
        assert_eq!(store.get(&hash(1)).await.unwrap(), None);

        // get_unchecked panics (errors) on a missing key.
        assert!(store.get_unchecked(&hash(9)).await.is_err());
    }

    /// **The regression test for AUDIT C122**, written against the *store* because that is the path C110's
    /// slash rule actually reads, and it asserts both halves of the finding: the flag survives, and the
    /// rule can therefore see it.
    ///
    /// `slashable` is what `validate::slashable_senders` consults to decide which validators a block's
    /// evidence holds responsible. Every read of a stored metadata goes `get` → codec `decode` →
    /// `BlockMetadata::from_proto`, and that conversion hard-coded `false` — so the flag was false for
    /// *every* metadata any caller could see, not only after a restart as the field's doc claimed, and
    /// the slash rule was unreachable: the proposer's `to_slash` was always empty and `slash_is_unjustified`
    /// treated every `Slash` as unjustified.
    ///
    /// The existing round-trip test above cannot see it: its fixture is built with `slashable: false`, so
    /// `false` round-trips to `false`. This one sets the flag — the only difference — and then hands the
    /// read-back metadata to the rule, because "the flag survives" is only interesting as "the rule is
    /// reachable".
    #[tokio::test]
    async fn the_slashable_flag_survives_the_store_round_trip_and_reaches_the_slash_rule() {
        let store = BlockMetadataStore::create(metadata_store()).await.unwrap();
        let mut attributable = meta(hash(0), &[], 0);
        attributable.validation_failed = true;
        attributable.slashable = true;
        store.add(attributable.clone()).await.unwrap();

        let read_back = store.get(&hash(0)).await.unwrap().expect("stored");
        assert!(
            read_back.slashable,
            "the flag the slash rule reads must survive the store — without it nothing in this tree can \
             take a bonded validator's stake (AUDIT C122)"
        );
        assert_eq!(
            read_back, attributable,
            "the whole metadata round-trips, not only the flag"
        );

        // The consequence, which is the finding: a stored attributable failure is evidence.
        assert_eq!(
            crate::validate::slashable_senders(&[read_back]),
            BTreeSet::from([attributable.sender]),
            "a validator that stored an attributable failure must be whom the slash rule names"
        );
    }

    #[tokio::test]
    async fn dag_state_tracks_child_and_height_maps() {
        let store = BlockMetadataStore::create(metadata_store()).await.unwrap();
        let genesis = meta(hash(0), &[], 0);
        let child = meta(hash(1), &[hash(0)], 1);
        store.add(genesis).await.unwrap();
        store.add(child).await.unwrap();

        let child_map = store.child_map_data().await;
        assert_eq!(child_map[&hash(0)], [hash(1)].into_iter().collect());
        assert!(child_map[&hash(1)].is_empty());

        let height_map = store.height_map().await;
        assert_eq!(
            height_map[&rchain_shared::refined::BlockHeight::try_from(0).unwrap()],
            [hash(0)].into_iter().collect()
        );
        assert_eq!(
            height_map[&rchain_shared::refined::BlockHeight::try_from(1).unwrap()],
            [hash(1)].into_iter().collect()
        );
    }
}
