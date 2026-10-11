//! Block random seed (port of `rholang/BlockRandomSeed.scala`).
//!
//! Deterministically derives a [`Blake2b512Random`] from a block's identity fields so that
//! deployment and genesis unforgeable names are reproducible.

use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::ast::Par;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_models::rholang::RhoType::RhoName;
use rchain_rholang::merging::vlong_encode;
use std::fmt;

/// Random-seed split index for the pre-charge deploy (port of `PreChargeSplitIndex`).
pub const PRE_CHARGE_SPLIT_INDEX: u8 = 0;
/// Random-seed split index for the user deploy (port of `UserDeploySplitIndex`).
pub const USER_DEPLOY_SPLIT_INDEX: u8 = 1;
/// Random-seed split index for the refund deploy (port of `RefundSplitIndex`).
pub const REFUND_SPLIT_INDEX: u8 = 2;

/// A block-random-seed component cannot be represented by the legacy 1-byte length prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockRandomSeedError {
    ComponentTooLong {
        component: &'static str,
        len: usize,
        max: usize,
    },
}

impl fmt::Display for BlockRandomSeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComponentTooLong {
                component,
                len,
                max,
            } => write!(
                f,
                "block random seed {component} is {len} bytes; the legacy encoding allows at most {max}"
            ),
        }
    }
}

impl std::error::Error for BlockRandomSeedError {}

/// A deterministic random seed derived from a block (port of `BlockRandomSeed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRandomSeed {
    pub shard_id: String,
    pub block_number: i64,
    pub sender: PublicKey,
    pub pre_state_hash: Blake2b256Hash,
}

impl BlockRandomSeed {
    /// The seed for a shard's block. **Asserts** the shard id is ASCII, in every profile.
    ///
    /// The oracle's own constructor asserts this (`BlockRandomSeed.scala:39`'s
    /// `assert(shardId.onlyAscii, …)`), so the port's `debug_assert!` was a *weaker* port: in a release
    /// build the check vanished and a non-ASCII shard id flowed into the seed that Law 11's determinism
    /// rests on. A domain check that is debug-only is not whitelistable (the audit's asymmetry rule).
    ///
    /// It is unreachable from the block path: `validate::format_of_fields` refuses an empty or
    /// non-ASCII shard id at ingress through `ShardId::try_from` (Law 26), with
    /// `format_of_fields_rejects_non_ascii_shard_id` as its witness in release. The direct callers pass
    /// config-derived ids, which `ShardSpec`/`ShardId` have already validated (C78). So this is the
    /// internal restatement the Scala also keeps, not a boundary check.
    pub fn new(
        shard_id: String,
        block_number: i64,
        sender: PublicKey,
        pre_state_hash: Blake2b256Hash,
    ) -> Self {
        assert!(
            shard_id.is_ascii(),
            "Shard name should contain only ASCII characters"
        );
        BlockRandomSeed {
            shard_id,
            block_number,
            sender,
            pre_state_hash,
        }
    }

    /// The genesis seed (port of `BlockRandomSeed.apply(shardId)`).
    pub fn from_shard_id(shard_id: &str) -> Self {
        BlockRandomSeed::new(
            shard_id.to_string(),
            0,
            PublicKey::new(vec![]),
            Blake2b256Hash::create(&[]),
        )
    }

    /// The seed for a block (port of `BlockRandomSeed.apply(block)`).
    pub fn from_block(block: &BlockMessage) -> Self {
        if block.justifications.is_empty() {
            BlockRandomSeed::from_shard_id(&block.shard_id)
        } else {
            BlockRandomSeed::new(
                block.shard_id.clone(),
                i64::from(block.block_number),
                PublicKey::new(block.sender.as_bytes().to_vec()),
                Blake2b256Hash::from_byte_array(block.pre_state_hash.as_bytes()),
            )
        }
    }

    /// Serialize the seed (port of the scodec `codecBlockRandomSeed`).
    pub fn encode(&self) -> Result<Vec<u8>, BlockRandomSeedError> {
        let mut bytes = var_size("shard id", self.shard_id.as_bytes())?;
        bytes.extend(vlong_encode(self.block_number));
        bytes.extend(var_size("sender public key", self.sender.bytes())?);
        bytes.extend_from_slice(self.pre_state_hash.as_bytes());
        Ok(bytes)
    }

    /// Derive the random generator from this seed (port of `randomGenerator(BlockRandomSeed)`).
    pub fn random_generator(&self) -> Result<Blake2b512Random, BlockRandomSeedError> {
        let encoded = self.encode()?;
        Ok(Blake2b512Random::from_init(&encoded))
    }

    /// `randomGenerator(shardId, blockNumber, sender, preStateHash)`.
    pub fn random_generator_from(
        shard_id: &str,
        block_number: i64,
        sender: PublicKey,
        pre_state_hash: Blake2b256Hash,
    ) -> Result<Blake2b512Random, BlockRandomSeedError> {
        BlockRandomSeed::new(shard_id.to_string(), block_number, sender, pre_state_hash)
            .random_generator()
    }

    /// `randomGenerator(shardId)` (genesis).
    pub fn random_generator_from_shard_id(
        shard_id: &str,
    ) -> Result<Blake2b512Random, BlockRandomSeedError> {
        BlockRandomSeed::from_shard_id(shard_id).random_generator()
    }

    /// `randomGenerator(block)`.
    pub fn random_generator_from_block(
        block: &BlockMessage,
    ) -> Result<Blake2b512Random, BlockRandomSeedError> {
        BlockRandomSeed::from_block(block).random_generator()
    }

    /// `splitRandomNumberFromGenesis`.
    pub fn split_random_number_from_genesis(
        shard_id: &str,
        index: u8,
        index2: u8,
    ) -> Result<Blake2b512Random, BlockRandomSeedError> {
        Ok(BlockRandomSeed::random_generator_from_shard_id(shard_id)?
            .split_byte(index)
            .split_byte(index2))
    }

    /// The unforgeable name of the NonNegativeNumber contract (port of
    /// `nonNegativeMergeableTagName`).
    pub fn non_negative_mergeable_tag_name(shard_id: &str) -> Result<Par, BlockRandomSeedError> {
        let mut rand = BlockRandomSeed::split_random_number_from_genesis(
            shard_id,
            3,
            USER_DEPLOY_SPLIT_INDEX,
        )?;
        rand.next();
        Ok(RhoName::apply_bytes(rand.next()))
    }

    /// The REV transfer unforgeable name (port of `transferUnforgeable`).
    pub fn transfer_unforgeable(shard_id: &str) -> Result<Par, BlockRandomSeedError> {
        let mut rand = BlockRandomSeed::split_random_number_from_genesis(
            shard_id,
            6,
            USER_DEPLOY_SPLIT_INDEX,
        )?;
        for _ in 0..10 {
            rand.next();
        }
        Ok(RhoName::apply_bytes(rand.next()))
    }

    /// The store-token unforgeable name (port of `storeTokenUnforgeable`).
    pub fn store_token_unforgeable(shard_id: &str) -> Result<Par, BlockRandomSeedError> {
        let mut rand = BlockRandomSeed::split_random_number_from_genesis(
            shard_id,
            0,
            USER_DEPLOY_SPLIT_INDEX,
        )?;
        for _ in 0..9 {
            rand.next();
        }
        Ok(RhoName::apply_bytes(rand.next()))
    }

    /// The REV vault unforgeable name (port of `revVaultUnforgeable`).
    pub fn rev_vault_unforgeable(shard_id: &str) -> Result<Par, BlockRandomSeedError> {
        let mut rand = BlockRandomSeed::split_random_number_from_genesis(
            shard_id,
            6,
            USER_DEPLOY_SPLIT_INDEX,
        )?;
        Ok(RhoName::apply_bytes(rand.next()))
    }
}

/// `variableSizeBytes(uint8, X)` — a 1-byte length prefix followed by the bytes.
fn var_size(component: &'static str, bytes: &[u8]) -> Result<Vec<u8>, BlockRandomSeedError> {
    let len = u8::try_from(bytes.len()).map_err(|_| BlockRandomSeedError::ComponentTooLong {
        component,
        len: bytes.len(),
        max: u8::MAX as usize,
    })?;
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(len);
    out.extend_from_slice(bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlong_encodes_zero_and_small_values() {
        assert_eq!(vlong_encode(0), vec![0x00]);
        assert_eq!(vlong_encode(1), vec![0x02]);
        assert_eq!(vlong_encode(5), vec![0x0a]);
    }

    #[test]
    fn valid_v1_encoding_bytes_are_unchanged() {
        let seed = BlockRandomSeed::new(
            "root".to_string(),
            1,
            PublicKey::new(vec![0xaa, 0xbb]),
            Blake2b256Hash::from_bytes([0x11; 32]),
        );
        let mut expected = vec![4, b'r', b'o', b'o', b't', 0x02, 2, 0xaa, 0xbb];
        expected.extend_from_slice(&[0x11; 32]);

        assert_eq!(seed.encode().unwrap(), expected);
    }

    #[test]
    fn seed_is_deterministic() {
        let seed = BlockRandomSeed::from_shard_id("root");
        assert_eq!(
            seed.encode().unwrap(),
            BlockRandomSeed::from_shard_id("root").encode().unwrap()
        );
        assert_ne!(
            seed.encode().unwrap(),
            BlockRandomSeed::new(
                "root".to_string(),
                1,
                PublicKey::new(vec![]),
                Blake2b256Hash::create(&[])
            )
            .encode()
            .unwrap()
        );
    }

    /// **H2d.** The ASCII check on the shard id must exist in **release**, not only in a debug build:
    /// the oracle's own constructor *asserts* it (`legacy/.../BlockRandomSeed.scala:39`), and a check
    /// that vanishes at `--release` refuses nothing there. A domain check that is `debug_assert!`-only
    /// is not whitelistable (the audit's asymmetry rule), so this pins it in **both** profiles by
    /// catching the panic: under a `debug_assert!` it fails with `--release`.
    #[test]
    fn a_non_ascii_shard_id_is_refused_in_any_profile() {
        let refused = std::panic::catch_unwind(|| {
            BlockRandomSeed::new(
                "røøt".to_string(),
                1,
                PublicKey::new(vec![]),
                Blake2b256Hash::create(&[]),
            )
        });
        assert!(
            refused.is_err(),
            "a non-ASCII shard id must be refused in release too"
        );

        // The control: an ASCII shard id is not refused, so the test can fail.
        let accepted = std::panic::catch_unwind(|| BlockRandomSeed::from_shard_id("root"));
        assert!(accepted.is_ok(), "an ASCII shard id is accepted");
    }

    #[test]
    fn oversized_sender_is_a_typed_error_not_a_panic() {
        let seed = BlockRandomSeed::new(
            "root".to_string(),
            1,
            PublicKey::new(vec![0; 256]),
            Blake2b256Hash::create(&[]),
        );

        let err = match seed.random_generator() {
            Ok(_) => panic!("an oversized sender must be refused"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            BlockRandomSeedError::ComponentTooLong {
                component: "sender public key",
                len: 256,
                max: 255,
            }
        );
    }

    #[test]
    fn oversized_shard_id_is_a_typed_error_not_a_panic() {
        let seed = BlockRandomSeed::new(
            "a".repeat(256),
            1,
            PublicKey::new(vec![]),
            Blake2b256Hash::create(&[]),
        );

        assert_eq!(
            seed.encode(),
            Err(BlockRandomSeedError::ComponentTooLong {
                component: "shard id",
                len: 256,
                max: 255,
            })
        );
    }

    #[test]
    fn random_generator_produces_names() {
        let name = BlockRandomSeed::non_negative_mergeable_tag_name("root").unwrap();
        assert!(!name.unforgeables.is_empty() || !name.exprs.is_empty());
    }
}
