//! A state hash.
//!
//! Mirrors `models/src/main/scala/coop/rchain/models/block/StateHash.scala`.
//!
//! The 32-byte storage is the shared [`Hash32`](rchain_shared::refined::Hash32) newtype.

use rchain_shared::refined::Hash32;

use crate::errors::ModelsError;

/// The length of a `StateHash` in bytes.
pub const LENGTH: usize = 32;

/// A 32-byte state hash.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StateHash(Hash32);

impl StateHash {
    pub fn new(bytes: [u8; LENGTH]) -> Self {
        Self(Hash32::new(bytes))
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), LENGTH, "expected {LENGTH} bytes");
        let mut arr = [0u8; LENGTH];
        arr.copy_from_slice(bytes);
        Self(Hash32::new(arr))
    }

    pub fn as_bytes(&self) -> &[u8; LENGTH] {
        self.0.as_bytes()
    }
}

impl TryFrom<&[u8]> for StateHash {
    type Error = ModelsError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != LENGTH {
            return Err(ModelsError::Length {
                got: bytes.len(),
                expected: LENGTH,
            });
        }
        Ok(Self::from_slice(bytes))
    }
}

/// Total conversion from the canonical digest type (both are fixed 32-byte wrappers).
impl From<rchain_crypto::hash::blake2b256_hash::Blake2b256Hash> for StateHash {
    fn from(h: rchain_crypto::hash::blake2b256_hash::Blake2b256Hash) -> Self {
        Self(h.into())
    }
}

impl From<StateHash> for rchain_crypto::hash::blake2b256_hash::Blake2b256Hash {
    fn from(h: StateHash) -> Self {
        h.0.into()
    }
}

impl From<Hash32> for StateHash {
    fn from(h: Hash32) -> Self {
        StateHash(h)
    }
}

impl From<StateHash> for Hash32 {
    fn from(h: StateHash) -> Self {
        h.0
    }
}

// **Serde, so a hash field can be typed rather than a `Vec<u8>` with a `#[serde(with = …)]`** (the
// deferred item 1b). The encoding is the one `models::ast::hex_serde` already produced —
// lowercase base16 — so the JSON a client sees is byte-for-byte what it was; what changes is that a
// wrong-length value is now refused at the boundary instead of reaching a `Vec<u8>` field that
// nothing checks. `Deserialize` goes through the length-checked `TryFrom`, not `from_slice`, because
// this is an ingress: a 31-byte hash in a request body must be an error rather than a panic.
impl serde::Serialize for StateHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&rchain_shared::base16::encode(self.as_bytes()))
    }
}

impl<'de> serde::Deserialize<'de> for StateHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let bytes = rchain_shared::base16::decode(&s)
            .ok_or_else(|| serde::de::Error::custom("invalid state hash hex"))?;
        StateHash::try_from(bytes.as_slice()).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(seed: u8) -> Vec<u8> {
        (0..LENGTH).map(|i| seed.wrapping_add(i as u8)).collect()
    }

    /// `TryFrom` is the **checked** path — validate-on-ingress — so a slice of the wrong length is an
    /// error that names both numbers, never a truncation and never a panic.
    #[test]
    fn try_from_rejects_a_wrong_length_and_names_both() {
        let ok = StateHash::try_from(bytes(7).as_slice()).expect("32 bytes");
        assert_eq!(
            ok.as_bytes(),
            &<[u8; LENGTH]>::try_from(bytes(7).as_slice()).unwrap()
        );

        for bad in [0usize, 31, 33, 64] {
            let err = StateHash::try_from(vec![0u8; bad].as_slice()).expect_err(&format!("{bad}"));
            assert_eq!(
                err,
                ModelsError::Length {
                    got: bad,
                    expected: LENGTH
                }
            );
            assert_eq!(err.to_string(), format!("expected 32 bytes, got {bad}"));
        }
    }

    /// `from_slice` is the infallible path: it *asserts* the caller's promise instead of silently
    /// truncating or zero-filling. That is the boundary the checked `TryFrom` exists for.
    #[test]
    #[should_panic(expected = "expected 32 bytes")]
    fn from_slice_asserts_the_length_rather_than_truncating() {
        StateHash::from_slice(&[0u8; 31]);
    }

    /// Every conversion route carries the same 32 bytes: `Hash32` (the shared storage newtype) and
    /// `Blake2b256Hash` (the crypto digest the trie hashes with) are views of the same value, so a
    /// state hash computed on one side compares equal on the other.
    #[test]
    fn the_conversions_keep_the_bytes() {
        let arr: [u8; LENGTH] = bytes(1).as_slice().try_into().expect("32");
        let hash = StateHash::new(arr);
        assert_eq!(hash.as_bytes(), &arr);

        let raw: Hash32 = hash.into();
        assert_eq!(raw.as_bytes(), &arr, "Hash32 keeps the bytes");
        assert_eq!(
            StateHash::from(raw),
            hash,
            "…and the round trip is the identity"
        );

        let digest: rchain_crypto::hash::blake2b256_hash::Blake2b256Hash = hash.into();
        assert_eq!(digest.as_bytes(), &arr, "Blake2b256Hash keeps the bytes");
        assert_eq!(StateHash::from(digest), hash);

        // `from_slice` and `new` agree, which is what makes the slice path safe to use internally.
        assert_eq!(StateHash::from_slice(&arr), hash);
    }

    /// **Deferred item 1b: the boundary that makes the typed fields worth having.** A report's
    /// `post_state_hash` was a `Vec<u8>` with a `#[serde(with = "hex_serde")]`, which accepted any
    /// hex of any length — a 31-byte hash in a request body became a 31-byte vector that nothing
    /// checked. The encoding asserted here is the one that attribute produced, so the JSON a client
    /// sees is unchanged; what changed is that a wrong-length or non-hex value is refused *at the
    /// boundary*, which is the only place it can be.
    #[test]
    fn the_serde_boundary_keeps_the_hex_and_refuses_a_wrong_length() {
        let hash = StateHash::new([0xab; LENGTH]);
        let json = serde_json::to_string(&hash).expect("serialize");
        assert_eq!(
            json,
            format!("\"{}\"", "ab".repeat(LENGTH)),
            "lowercase base16, the same string the hex_serde attribute wrote"
        );
        assert_eq!(
            serde_json::from_str::<StateHash>(&json).expect("round trip"),
            hash
        );

        // The three refusals: empty, a short hex, and a hex of the right shape but the wrong length.
        for bad in [
            "\"\"".to_string(),
            "\"ab\"".to_string(),
            format!("\"{}\"", "ab".repeat(LENGTH - 1)),
        ] {
            assert!(
                serde_json::from_str::<StateHash>(&bad).is_err(),
                "a value that is not a 32-byte hash must not deserialize: {bad}"
            );
        }
        assert!(
            serde_json::from_str::<StateHash>("\"zz\"").is_err(),
            "and neither must one that is not hex at all"
        );
    }

    /// Equality, ordering and hashing all come from the bytes, and the type is `Copy` — the
    /// properties the DAG and the block store rely on when they key maps by state hash.
    #[test]
    fn the_hash_compares_by_its_bytes() {
        let low = StateHash::new([0u8; LENGTH]);
        let mut high_bytes = [0u8; LENGTH];
        high_bytes[0] = 1;
        let high = StateHash::new(high_bytes);

        assert_eq!(low, StateHash::new([0u8; LENGTH]));
        assert_ne!(low, high);
        assert!(low < high, "lexicographic over the bytes");
        assert_eq!(low.min(high), low);

        let copied = high;
        assert_eq!(copied, high, "`Copy`, not a move");

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let h = |v: &StateHash| {
            let mut s = DefaultHasher::new();
            v.hash(&mut s);
            s.finish()
        };
        assert_eq!(h(&low), h(&StateHash::new([0u8; LENGTH])));
    }
}
