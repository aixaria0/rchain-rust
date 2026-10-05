//! Session identity: the OCapN **Public Identifier** and **Session ID**.
//!
//! `draft-specifications/CapTP Specification.md` fixes both as 32-byte arrays and fixes the
//! derivation exactly, so this module is a transcription of six steps and nothing else:
//!
//! * **Public Identifier** — 32 bytes: "Serialize the per session public key", "SHA256 hash of the
//!   result", "SHA256 hash of the result produced in step 2" (two rounds).
//! * **Session ID** — 32 bytes: take each side's Public Identifier, "Sort both IDs based on the
//!   resulting octets", "Concatinate the Public Identifiers in the order determined", "Prepend the
//!   string `\"prot0\"` to the beginning", then two SHA-256 rounds. Sorting by octets means the two
//!   peers agree on the id without agreeing on who is "first".
//!
//! The constants are pinned by known-answer vectors computed from the derivation
//! ([`public_identifier`]/[`session_id`]'s tests), because the *composition* — the sort order, the
//! `prot0` prefix, the number of rounds — is the part that would interoperate with nobody while
//! passing every round-trip test locally.

use self::CrossedHello::{Abort, Keep};

/// A 32-byte digest — the fixed width of both the Public Identifier and the Session ID.
pub type Octets32 = [u8; 32];

/// SHA-256, coerced to the fixed 32 bytes the spec guarantees. SHA-256 is always 32 bytes, so the
/// copy is total; the `zip` keeps it so without an `unwrap` (this crate forbids panics on the
/// production path).
fn sha256_32(input: &[u8]) -> Octets32 {
    let digest = rchain_crypto::hash::sha256::hash(input);
    let mut out = [0u8; 32];
    for (slot, byte) in out.iter_mut().zip(digest) {
        *slot = byte;
    }
    out
}

/// The Public Identifier of a session: two SHA-256 rounds over the serialized session public key.
pub fn public_identifier(serialized_session_pubkey: &[u8]) -> Octets32 {
    sha256_32(&sha256_32(serialized_session_pubkey))
}

/// The Session ID shared by two peers: `SHA256(SHA256("prot0" ‖ sorted(PI_a, PI_b)))`.
pub fn session_id(a_serialized_pubkey: &[u8], b_serialized_pubkey: &[u8]) -> Octets32 {
    let mut ids = [
        public_identifier(a_serialized_pubkey),
        public_identifier(b_serialized_pubkey),
    ];
    ids.sort_unstable();
    let mut buf = Vec::with_capacity(5 + 64);
    buf.extend_from_slice(b"prot0");
    buf.extend_from_slice(&ids[0]);
    buf.extend_from_slice(&ids[1]);
    sha256_32(&sha256_32(&buf))
}

/// What a peer does when both sides dial each other at once ("crossed hellos").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossedHello {
    /// This peer's Public Identifier is the higher of the two: keep the connection.
    Keep,
    /// This peer's Public Identifier is the lower of the two: abort this connection.
    Abort,
}

/// The crossed-hello rule: "The lower of the two has its connection aborted. The higher of the two
/// should continue to be the valid session for the two peers."
///
/// Both peers see the same pair of Public Identifiers and so reach opposite, consistent decisions.
/// The degenerate case of two *equal* identifiers (a peer dialling itself) is not covered by the
/// spec; it aborts, which is the safe reading of "the lower of the two is aborted".
pub fn crossed_hello(own_pi: &Octets32, peer_pi: &Octets32) -> CrossedHello {
    if own_pi > peer_pi {
        Keep
    } else {
        Abort
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_shared::base16;

    /// Two distinct, easy-to-name 32-byte session public keys for the vectors.
    const KEY_A: [u8; 32] = [0x01; 32];
    const KEY_B: [u8; 32] = [0x02; 32];

    #[test]
    fn public_identifier_is_two_sha256_rounds() {
        // sha256(sha256(0x01 * 32)), computed independently of this code.
        assert_eq!(
            base16::encode(&public_identifier(&KEY_A)),
            "a0d4a0b8484643488c45836275bdcf2ca1bf542239aa6ba72bbc5a5951cfb044"
        );
        // sha256(sha256(b"")) — the second round is what makes this not the plain SHA-256 of the
        // empty string (e3b0c442…), so the round count is pinned and not merely the primitive.
        assert_eq!(
            base16::encode(&public_identifier(b"")),
            "5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456"
        );
        assert_ne!(
            base16::encode(&public_identifier(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn session_id_is_the_pinned_composition() {
        // sha256(sha256("prot0" ‖ sorted(PI(A), PI(B)))), computed independently of this code.
        assert_eq!(
            base16::encode(&session_id(&KEY_A, &KEY_B)),
            "14df3ceff3067cb55ee1588b022e5203dc6ed57e361185446304a13182fd6a17"
        );
    }

    #[test]
    fn session_id_is_symmetric_in_its_two_sides() {
        assert_eq!(session_id(&KEY_A, &KEY_B), session_id(&KEY_B, &KEY_A));
    }

    #[test]
    fn session_id_depends_on_the_sorted_pair_not_the_roles() {
        // PI(A) is the *higher* identifier and PI(B) the lower, so a role-ordered concatenation
        // would have produced a different id. Sorting is what the vector above pins.
        let pa = public_identifier(&KEY_A);
        let pb = public_identifier(&KEY_B);
        assert!(pa > pb, "the fixture relies on A > B after sorting");
        assert_ne!(session_id(&KEY_A, &KEY_B), session_id(&KEY_A, &KEY_A));
    }

    #[test]
    fn crossed_hello_aborts_the_lower_identifier() {
        let pa = public_identifier(&KEY_A); // higher
        let pb = public_identifier(&KEY_B); // lower
        assert_eq!(crossed_hello(&pa, &pb), Keep);
        assert_eq!(crossed_hello(&pb, &pa), Abort);
        // Opposite and consistent: exactly one side keeps the connection.
        assert_ne!(crossed_hello(&pa, &pb), crossed_hello(&pb, &pa));
    }

    #[test]
    fn crossed_hello_on_equal_identifiers_aborts() {
        let pi = public_identifier(&KEY_A);
        assert_eq!(crossed_hello(&pi, &pi), Abort);
    }
}
