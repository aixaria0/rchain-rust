//! Base32 — RFC 4648's alphabet in **lowercase, without padding**.
//!
//! **Why this port needs it, and why it is not `base16`.** Endo names a peer by
//! `base32(Ed25519 verifying key)` in an OCapN location's `designator`, and its websocket netlayer
//! *decodes* that field with its own base32 rather than treating it as opaque — a hex designator
//! throws `Invalid base32 character` there. So a node that advertises itself in hex is not merely
//! unconventional, it is unparseable to the one published peer that speaks this transport, and the
//! session it establishes never resolves for the location the peer dialled (AUDIT C245).
//!
//! The lowercase, unpadded shape is Endo's own (`BASE32_ALPHABET` in its websocket netlayer), and the
//! vectors below are RFC 4648 §10's, which that alphabet shares apart from case.

/// RFC 4648's alphabet, lowercased: Endo's `BASE32_ALPHABET`, character for character.
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Encode bytes as lowercase, unpadded base32.
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
        // Keep only the bits not yet emitted, so the accumulator cannot drift past a byte's worth.
        buffer &= (1 << bits) - 1;
    }
    // The tail is fewer than five bits: shift them up into the low five and emit one character.
    // Padding is never written — Endo's encoder does not emit it and its decoder does not expect it.
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// Decode lowercase, unpadded base32, failing on any other character.
///
/// Rejects rather than repairs: a character outside the alphabet, or a length no number of bytes can
/// have produced, is `None`. Trailing bits that are not zero are rejected too, so two different
/// encodings can never decode to the same bytes.
pub fn decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut buffer: u64 = 0;
    let mut bits: u32 = 0;
    for character in input.bytes() {
        let value = ALPHABET.iter().position(|c| *c == character)? as u64;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    // Whatever is left is the tail of the last byte: fewer than 8 bits, and they have to be zero
    // (RFC 4648 §3.5). A non-zero tail is a different encoding of the same bytes, and accepting it
    // would make the decoder accept strings the encoder never writes.
    if bits > 0 && (buffer & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10's vectors, lowercased — the alphabet aside from case is the one Endo uses.
    #[test]
    fn the_rfc_4648_vectors_encode_and_round_trip() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "my"),
            ("fo", "mzxq"),
            ("foo", "mzxw6"),
            ("foob", "mzxw6yq"),
            ("fooba", "mzxw6ytb"),
            ("foobar", "mzxw6ytboi"),
        ] {
            assert_eq!(encode(plain.as_bytes()), encoded, "encoding {plain:?}");
            assert_eq!(
                decode(encoded),
                Some(plain.as_bytes().to_vec()),
                "decoding {encoded:?}"
            );
        }
    }

    /// **The case that matters here**: a 32-byte key is 52 characters (256 bits / 5, rounded up), which
    /// is what a peer's designator holds.
    #[test]
    fn a_thirty_two_byte_key_is_fifty_two_characters() {
        let key: Vec<u8> = (0u8..32).collect();
        let encoded = encode(&key);
        assert_eq!(encoded.len(), 52);
        assert_eq!(decode(&encoded), Some(key));
    }

    /// Non-alphabet characters, uppercase, and padded input are all refused — this decoder is used on
    /// a peer's designator, so it must not accept a second spelling of the same key.
    #[test]
    fn only_the_lowercase_unpadded_alphabet_decodes() {
        assert_eq!(decode("mzxw6ytboi="), None, "padding is not this alphabet");
        assert_eq!(
            decode("MZXW6YTBOI"),
            None,
            "uppercase is a different string"
        );
        assert_eq!(decode("mzxw6ytboi!"), None, "not in the alphabet");
        assert_eq!(decode("my1"), None, "not in the alphabet");
        // `mzxw6` decodes to `foo`; `mzxw7` has a non-zero tail and decodes to nothing.
        assert_eq!(decode("mzxw7"), None, "a non-zero tail is refused");
    }
}
