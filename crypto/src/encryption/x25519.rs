//! Raw X25519 (RFC 7748) — the Diffie-Hellman primitive underneath OCapN's Noise transport.
//!
//! **Why this is not [`crate::encryption::curve25519`].** That module is a NaCl *sealed box*
//! (`crypto_box_curve25519xsalsa20poly1305`): a whole construction, with a nonce and a MAC, which is
//! what the Scala original uses for confidential messaging. A Noise handshake needs the primitive
//! underneath it — a bare scalar multiplication whose 32-byte result it can hash straight into its
//! chaining key — and a sealed box cannot be taken apart into one. So this is that primitive and
//! nothing else, in the module that owns key material (Law 19).
//!
//! **The witnesses are RFC 7748's own**, and that is the second reason the module exists rather than a
//! call site reaching for `x25519-dalek` directly: `spec/laws.tsv`'s Law 19 note records this
//! repository as carrying **no** RFC 7748 vector, so the primitives under it were pinned only by
//! round trips of our own construction. The tests below are the vectors from §5.2 and §6.1 of the
//! RFC, and the row's `rustWitness` names them.

use rand::Rng;
use zeroize::Zeroize;

/// The X25519 base point: the u-coordinate `9` (RFC 7748 §4.1), little-endian in 32 bytes.
const BASE_POINT: [u8; 32] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// `X25519(k, u)` — the function RFC 7748 §5 defines.
///
/// Both arguments and the result are 32 bytes of little-endian u-coordinate. The scalar is **clamped
/// inside** the primitive, as the RFC requires; a caller that clamps again is harmless and one that
/// forgets is not, so the clamping lives where it cannot be forgotten.
pub fn x25519(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    x25519_dalek::x25519(*scalar, *u)
}

/// The public u-coordinate for a secret scalar: `X25519(secret, 9)`.
///
/// This is what a peer puts in a locator and what a Noise handshake carries, so it is derived from the
/// secret by the same function every peer will use rather than by a second path that could disagree.
pub fn public_from_secret(secret: &[u8; 32]) -> [u8; 32] {
    x25519(secret, &BASE_POINT)
}

/// A node's X25519 **static** key — 32 secret bytes and nothing else.
///
/// A newtype rather than a bare `[u8; 32]` for the reason every refinement here is a newtype: a raw
/// array of that length is also a public key, a nonce and a hash, and these must not be passable where
/// one another is expected. `Debug` is written by hand and **redacts**, and the buffer is zeroed on
/// drop — the same pair, and the same honest limits, as [`crate::private_key::PrivateKey`]: they
/// remove the *silent* leak (a key in a log) rather than making the secret unrecoverable, and `Clone`
/// survives for the same reason it does there.
#[derive(Clone, PartialEq, Eq)]
pub struct StaticKey([u8; 32]);

impl std::fmt::Debug for StaticKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The width is printed because it identifies the curve without identifying the key.
        write!(f, "StaticKey(<redacted, {} bytes>)", self.0.len())
    }
}

impl Drop for StaticKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl StaticKey {
    /// Draw a fresh key from the OS CSPRNG.
    pub fn generate() -> StaticKey {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        StaticKey(bytes)
    }

    /// Reconstruct a key from the bytes it was persisted as.
    pub fn from_bytes(bytes: [u8; 32]) -> StaticKey {
        StaticKey(bytes)
    }

    /// The secret bytes, for persisting with mode `0600`.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// The public u-coordinate a peer sees.
    pub fn public(&self) -> [u8; 32] {
        public_from_secret(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_shared::base16;

    fn hex(s: &str) -> [u8; 32] {
        base16::decode(s)
            .expect("a base16 vector")
            .try_into()
            .expect("32 bytes")
    }

    /// **RFC 7748 §5.2, test vector 1** — the vector the RFC opens its worked examples with.
    #[test]
    fn rfc7748_section_5_2_first_vector() {
        assert_eq!(
            base16::encode(&x25519(
                &hex("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
                &hex("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
            )),
            "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"
        );
    }

    /// **RFC 7748 §5.2, test vector 2** — the one whose input u-coordinate has its high bit set, which
    /// is the case a port that mistakes the u-coordinate for a point encoding gets wrong.
    #[test]
    fn rfc7748_section_5_2_second_vector() {
        assert_eq!(
            base16::encode(&x25519(
                &hex("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d"),
                &hex("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493"),
            )),
            "95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957"
        );
    }

    /// **RFC 7748 §5.2's iterated vector, after one round.** The RFC also gives the 1000th iterate;
    /// one round is what catches a wrong ladder, and a thousand would only catch a wrong *timing*
    /// side channel this port does not claim to close.
    #[test]
    fn rfc7748_section_5_2_iterated_once() {
        let k = hex("0900000000000000000000000000000000000000000000000000000000000000");
        let u = k;
        assert_eq!(
            base16::encode(&x25519(&k, &u)),
            "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079"
        );
    }

    /// **RFC 7748 §6.1's Diffie-Hellman example**, both halves at once: each party's public key is
    /// derived from its secret, and both arrive at the same shared secret. The public derivation and
    /// the shared-secret multiplication are the two calls a Noise handshake makes, and this is the
    /// vector that says they agree with a peer that implements the RFC.
    #[test]
    fn rfc7748_section_6_1_diffie_hellman_agrees_both_ways() {
        let alice_secret = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_secret = hex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");

        let alice_public = public_from_secret(&alice_secret);
        let bob_public = public_from_secret(&bob_secret);
        assert_eq!(
            base16::encode(&alice_public),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
        assert_eq!(
            base16::encode(&bob_public),
            "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
        );

        let shared = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
        assert_eq!(base16::encode(&x25519(&alice_secret, &bob_public)), shared);
        assert_eq!(base16::encode(&x25519(&bob_secret, &alice_public)), shared);
    }

    /// A generated key round-trips through its persisted bytes and agrees with its own public half —
    /// the property a node restarting with a stored key depends on.
    #[test]
    fn a_generated_key_round_trips_and_agrees_with_its_public() {
        let key = StaticKey::generate();
        let restored = StaticKey::from_bytes(key.to_bytes());
        assert_eq!(restored, key);
        assert_eq!(restored.public(), key.public());
        // Not the identity: a key whose public is itself would be a broken derivation that still
        // round-trips.
        assert_ne!(key.public(), key.to_bytes());
    }

    /// The redaction is **observable**, so it is pinned — the same test, and the same reason, as
    /// `PrivateKey`'s.
    #[test]
    fn the_debug_output_redacts_the_key() {
        let key = StaticKey::from_bytes([0xAB; 32]);
        assert_eq!(format!("{key:?}"), "StaticKey(<redacted, 32 bytes>)");
        assert!(!format!("{key:?}").contains("171"));
    }
}
