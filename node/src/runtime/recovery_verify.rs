//! `rnode verify-block` and `rnode verify-attestation` — the reconciliation tool's two verifiers.
//!
//! **Why these are subcommands of the node rather than code in the tool.** The recovery utility
//! (`tools/reconcile-network.sh`) is a shell script, and the meet it computes must not become a
//! *second* implementation of the node's rules: the codebase's own warning is that two
//! implementations of one rule can disagree (`Split`, law 51b — the note on `NoAdvance` in
//! `block-storage/src/dag/finalizer.rs`). So the split is deliberate:
//!
//! - the **finality** rule stays in the node entirely — an attestation *is* the node's own finaliser
//!   saying which block it has finalised, so nothing client-side re-derives it;
//! - the two checks a client genuinely needs — *is this hash the hash of these bytes*, and *did this
//!   key sign these 32 bytes* — live in `casper::proto_util::hash_block` and
//!   `casper::validate::block_signature`. This module calls those, and the tool calls this module.
//!
//! Both read a node's JSON response on **stdin** and write a verdict on stdout: a JSON object on
//! success (exit 0), a one-line refusal on stderr with exit 1. The tool needs the *fields*, not just
//! a boolean — a hash that verifies is what makes the height, the justifications and the bond map in
//! the same bytes trustworthy, and those are what the meet's arithmetic is built from.

use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::signatures::signatures_alg::from_algorithm;
use rchain_models::block_hash::BlockHash;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_models::casper::protocol::deploy_service::{FinalityAttestation, RawBlock};
use rchain_shared::base16;
use serde_json::json;

use rchain_casper::api::block_api::bond_to_bond_info;
use rchain_casper::proto_util::hash_block;
use rchain_casper::validator_identity::attestation_prehash;

/// Verify a `RawBlock` response and print the fields it proves, or refuse.
///
/// The three checks, in the order that makes each meaningful:
///
/// 1. `blake2b256(encoded) == blockHash` — so the bytes are the preimage of the hash the response
///    claims. Before this, every field in a `LightBlockInfo` was the node's word.
/// 2. `sig` verifies over the 32 bytes of `blockHash` under `sender` — so a bonded key really did
///    sign this block, which is what makes the proposer attributable.
/// 3. the preimage decodes as a `BlockMessage` whose own `sender` is the one claimed — a
///    cross-check between the verified bytes and the response's separate `sender` field.
///
/// Only after all three does the height, seq_num, justifications and bond map get printed: they come
/// from the verified bytes, not from the response's summary of them.
pub fn verify_block(json_text: &str) -> Result<String, String> {
    let raw: RawBlock =
        serde_json::from_str(json_text).map_err(|e| format!("not a RawBlock response: {e}"))?;
    let bytes = base16::decode(&raw.encoded)
        .ok_or_else(|| "the response's `encoded` is not base16".to_string())?;

    let computed = Blake2b256Hash::create(&bytes);
    let hash = BlockHash::from(computed);
    if base16::encode(hash.as_bytes()) != raw.block_hash.to_lowercase() {
        return Err(format!(
            "REFUSED: the preimage hashes to {} but the response claims {}",
            base16::encode(hash.as_bytes()),
            raw.block_hash
        ));
    }

    let sig =
        base16::decode(&raw.sig).ok_or_else(|| "the response's `sig` is not base16".to_string())?;
    let sender = base16::decode(&raw.sender)
        .ok_or_else(|| "the response's `sender` is not base16".to_string())?;
    let alg = from_algorithm(&raw.sig_algorithm)
        .ok_or_else(|| format!("unknown signature algorithm {}", raw.sig_algorithm))?;
    if !alg.verify(hash.as_bytes(), &sig, &sender) {
        return Err(format!(
            "REFUSED: `sig` does not verify over the block hash under sender {}",
            raw.sender
        ));
    }

    let block = BlockMessage::from_bytes(&bytes)
        .map_err(|e| format!("the verified preimage does not decode as a block: {e}"))?;
    if base16::encode(block.sender.as_bytes()) != raw.sender.to_lowercase() {
        return Err(format!(
            "REFUSED: the preimage names sender {} but the response claims {}",
            base16::encode(block.sender.as_bytes()),
            raw.sender
        ));
    }
    // Re-deriving the hash from the decoded block is the closest thing to a round-trip the API
    // permits, and it is what catches a `to_bytes_cleared` that stopped clearing (or stopped being
    // the preimage) rather than the client's arithmetic being wrong.
    let rederived = hash_block(&block);
    if rederived != hash {
        return Err(format!(
            "REFUSED: the decoded block hashes to {} but its own preimage hashes to {} — the two \
             halves of this response disagree",
            base16::encode(rederived.as_bytes()),
            base16::encode(hash.as_bytes())
        ));
    }

    let verdict = json!({
        "verified": true,
        "blockHash": base16::encode(hash.as_bytes()),
        "blockNumber": i64::from(block.block_number),
        "seqNum": i64::from(block.seq_num),
        "sender": raw.sender.to_lowercase(),
        "postStateHash": base16::encode(block.post_state_hash.as_bytes()),
        "justifications": block
            .justifications
            .iter()
            .map(|h| base16::encode(h.as_bytes()))
            .collect::<Vec<_>>(),
        "bonds": block
            .bonds
            .iter()
            .map(|(v, s)| {
                let info = bond_to_bond_info((v, *s));
                json!({ "validator": info.validator, "stake": info.stake })
            })
            .collect::<Vec<_>>(),
    });
    Ok(verdict.to_string())
}

/// Verify a `FinalityAttestation` response against **the nonce this run asked for**, and print what
/// it proves.
///
/// The `expected_nonce` is the caller's, not the response's, and that is the point: a verifier that
/// took the nonce out of the response would accept a captured attestation replayed from any earlier
/// conversation. The response's own `nonce` is required to *equal* the one asked for, and the
/// signature is checked over the prehash built from the expected nonce — so a mismatch is a refusal
/// rather than a different, still-valid statement.
pub fn verify_attestation(json_text: &str, expected_nonce: &str) -> Result<String, String> {
    let attestation: FinalityAttestation = serde_json::from_str(json_text)
        .map_err(|e| format!("not a FinalityAttestation response: {e}"))?;

    if attestation.nonce.to_lowercase() != expected_nonce.to_lowercase() {
        return Err(format!(
            "REFUSED: the attestation answers nonce {} — this run asked for {expected_nonce}, so it \
             is either stale or not an answer to this question",
            attestation.nonce
        ));
    }
    let nonce = base16::decode(expected_nonce)
        .ok_or_else(|| format!("the nonce is not base16: {expected_nonce}"))?;
    let hash_bytes = base16::decode(&attestation.block_hash)
        .ok_or_else(|| "the attestation's `blockHash` is not base16".to_string())?;
    let hash = BlockHash::try_from(hash_bytes.as_slice()).map_err(|e| e.to_string())?;
    let sig = base16::decode(&attestation.sig)
        .ok_or_else(|| "the attestation's `sig` is not base16".to_string())?;
    let sender = base16::decode(&attestation.sender)
        .ok_or_else(|| "the attestation's `sender` is not base16".to_string())?;
    let alg = from_algorithm(&attestation.sig_algorithm)
        .ok_or_else(|| format!("unknown signature algorithm {}", attestation.sig_algorithm))?;

    let prehash = attestation_prehash(&nonce, &hash);
    if !alg.verify(&prehash, &sig, &sender) {
        return Err(format!(
            "REFUSED: the attestation for block {} is not signed by {} over this nonce",
            attestation.block_hash, attestation.sender
        ));
    }

    // **What this proves, and what it does not, written into the artefact the operator reads**
    // (W-N9-08).
    //
    // The row: a Byzantine validator can sign "I finalised X" for a block the network never finalised,
    // and this verifier accepts it — because it checks the signature over the prehash and nothing else.
    // It *cannot* check the claim, and pretending otherwise would be the worse defect: this is an
    // offline tool holding one JSON document, with no chain, no bond map and no quorum of
    // attestations. What is left is to stop the verdict from reading as though it had: the signature
    // covers a *statement*, and the statement is named here, beside the three things this tool does not
    // establish. An operator (or `tools/reconcile-network.sh`, whose meet weighs these) then reads the
    // claim rather than a bare `verified`.
    let verdict = json!({
        "verified": true,
        "proves": "this sender signed this block hash over this nonce",
        "doesNotProve": [
            "that the block was finalised — the network may never have seen it",
            "that the sender is bonded on the shard a recovery reads",
            "that a quorum agrees: one attestation is one validator's claim, and a claim can be false",
        ],
        "blockHash": attestation.block_hash.to_lowercase(),
        "sender": attestation.sender.to_lowercase(),
        "nonce": expected_nonce.to_lowercase(),
    });
    Ok(verdict.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_crypto::signatures::secp256k1::Secp256k1;
    use rchain_crypto::signatures::signatures_alg::SignaturesAlg;
    use rchain_models::casper::protocol::casper_message::BlockMessage;
    use rchain_models::validator::Validator;
    use rchain_shared::refined::{BlockHeight, SeqNum};

    use rchain_casper::validator_identity::ValidatorIdentity;

    /// The same key the casper-side attestation test uses, so a failure here points at the verifier
    /// rather than at key material.
    fn identity() -> ValidatorIdentity {
        ValidatorIdentity::from_hex(
            "67e56582298859ddae725f972992a07c6c4fb9f62a8fff58ce3ca926a1063530",
        )
        .unwrap()
    }

    fn a_signed_block(identity: &ValidatorIdentity) -> BlockMessage {
        let unsigned = rchain_casper::proto_util::unsigned_block_proto(
            1,
            "root".to_string(),
            BlockHeight::try_from(7).unwrap(),
            Validator::from_slice(&identity.public_key.bytes()),
            SeqNum::try_from(7).unwrap(),
            rchain_models::block::state_hash::StateHash::new([1u8; 32]),
            rchain_models::block::state_hash::StateHash::new([2u8; 32]),
            vec![BlockHash::new([3u8; 32])],
            std::collections::BTreeMap::from([(
                Validator::from_slice(&identity.public_key.bytes()),
                rchain_shared::refined::NonNegI64::try_from(100).unwrap(),
            )]),
            std::collections::BTreeSet::new(),
            rchain_models::casper::protocol::casper_message::RholangState::default(),
            0,
        );
        identity.sign_block(&unsigned).unwrap()
    }

    fn raw_response(block: &BlockMessage) -> String {
        let raw = RawBlock {
            block_hash: base16::encode(block.block_hash.as_bytes()),
            encoded: base16::encode(&block.to_bytes_cleared()),
            sender: base16::encode(block.sender.as_bytes()),
            sig_algorithm: block.sig_algorithm.clone(),
            sig: base16::encode(&block.sig),
        };
        serde_json::to_string(&raw).unwrap()
    }

    /// **The verifier's positive case, and what it prints.** A block the node really signed yields
    /// its height, seq_num, justifications and bond map — the fields the meet's arithmetic uses — and
    /// they come out of the bytes that hashed to the claimed hash.
    #[test]
    fn a_signed_block_verifies_and_yields_the_fields_the_meet_needs() {
        let identity = identity();
        let block = a_signed_block(&identity);
        let verdict = verify_block(&raw_response(&block)).expect("a real block verifies");
        let parsed: serde_json::Value = serde_json::from_str(&verdict).unwrap();

        assert_eq!(parsed["verified"], json!(true));
        assert_eq!(parsed["blockNumber"], json!(7));
        assert_eq!(parsed["seqNum"], json!(7));
        assert_eq!(
            parsed["blockHash"].as_str().unwrap(),
            base16::encode(block.block_hash.as_bytes())
        );
        assert_eq!(parsed["justifications"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["bonds"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["bonds"][0]["stake"], json!(100));
        assert_eq!(
            parsed["bonds"][0]["validator"].as_str().unwrap(),
            base16::encode(identity.public_key.bytes())
        );
    }

    /// **One byte of the preimage, and the whole thing falls over.** This is the check the summary
    /// route could never offer: a node that changes a block's height (or a justification, or a bond)
    /// so the summary says what it wants produces a response that no longer hashes to the hash it
    /// also reports, and the refusal names both.
    #[test]
    fn a_tampered_field_in_the_preimage_is_refused() {
        let identity = identity();
        let block = a_signed_block(&identity);
        let mut raw: serde_json::Value =
            serde_json::from_str(&raw_response(&block)).expect("the fixture is a response");

        // Rewrite the *preimage* — as a lying node would have to, since the hash and the signature
        // are over it — keeping the response's claimed hash.
        let mut bytes = block.to_bytes_cleared();
        let pos = bytes.len() / 2;
        bytes[pos] ^= 0x01;
        raw["encoded"] = json!(base16::encode(&bytes));

        let err = verify_block(&raw.to_string()).expect_err("a changed preimage cannot verify");
        assert!(
            err.contains("REFUSED") && err.contains("hashes to"),
            "the refusal names the mismatch: {err}"
        );
    }

    /// A response whose signature was produced by *another* key is refused, and so is one whose
    /// signature is over another hash: the two halves of the claim are checked separately.
    #[test]
    fn a_signature_by_another_key_or_over_another_hash_is_refused() {
        let identity = identity();
        let block = a_signed_block(&identity);
        let other = ValidatorIdentity::from_hex(
            "b71c71a67e1177ad4e901695e1b4b9ee17ae16c6668d313eac2f96dbcda3f291",
        )
        .unwrap();

        // Re-sign the same preimage with a different key, and claim the original sender.
        let other_sig = Secp256k1.sign(block.block_hash.as_bytes(), other.private_key.bytes());
        let mut raw: serde_json::Value =
            serde_json::from_str(&raw_response(&block)).expect("the fixture is a response");
        raw["sig"] = json!(base16::encode(&other_sig.unwrap()));
        let err =
            verify_block(&raw.to_string()).expect_err("another key's signature is not this key's");
        assert!(err.contains("does not verify"), "{err}");

        // The original signature, but a response claiming a different block hash.
        let mut raw2: serde_json::Value =
            serde_json::from_str(&raw_response(&block)).expect("the fixture is a response");
        raw2["blockHash"] = json!(base16::encode(&[9u8; 32]));
        let err2 =
            verify_block(&raw2.to_string()).expect_err("the hash and the preimage must agree");
        assert!(err2.contains("REFUSED"), "{err2}");
    }

    /// **The freshness rule, stated as a refusal.** An attestation the node signed for one nonce is
    /// not an answer to a run that asked with another — which is the property that makes a captured
    /// reply useless, and the reason the nonce is signed rather than echoed.
    #[test]
    fn an_attestation_for_another_nonce_is_refused() {
        let identity = identity();
        let block = a_signed_block(&identity);
        let nonce = "aa".repeat(16);
        let sig = identity
            .attest_finality(&base16::decode(&nonce).unwrap(), &block.block_hash)
            .unwrap();
        let attestation = FinalityAttestation {
            block_hash: base16::encode(block.block_hash.as_bytes()),
            nonce: nonce.clone(),
            sender: base16::encode(identity.public_key.bytes()),
            sig_algorithm: identity.sig_algorithm.clone(),
            sig: base16::encode(&sig),
        };
        let text = serde_json::to_string(&attestation).unwrap();

        // The nonce it was made for: verified, and it names the block and the signer.
        let verdict = verify_attestation(&text, &nonce).expect("the right nonce verifies");
        let parsed: serde_json::Value = serde_json::from_str(&verdict).unwrap();
        assert_eq!(parsed["verified"], json!(true));
        assert_eq!(
            parsed["blockHash"].as_str().unwrap(),
            base16::encode(block.block_hash.as_bytes())
        );

        // Another run's nonce: refused on the echo mismatch.
        let other = "bb".repeat(16);
        let err =
            verify_attestation(&text, &other).expect_err("a stale attestation is not an answer");
        assert!(err.contains("REFUSED") && err.contains("nonce"), "{err}");
    }

    /// **A signature over a claim is not the claim** (W-N9-08).
    ///
    /// The row's construction: a validator signs "I finalised X" for a block the network never
    /// finalised. Here that is a **real key over a hash nothing in this tree ever produced**, which is
    /// the strongest form of the lie — and the tool accepts it, because a signature over
    /// `blake2b256(tag‖nonce‖hash)` is all it has ever checked.
    ///
    /// The arm pins both halves. The acceptance is the row (so the cell states the deviation rather
    /// than a fix that did not happen), and the verdict's own words are the closure: it names the
    /// statement the signature covers and the three it does not establish. A verifier that cannot
    /// check a claim must not answer as though it had — this is an offline tool with one JSON document,
    /// no chain, no bond map and no quorum, and the artefact now says so.
    ///
    /// Falsified by dropping `doesNotProve` (or by a verdict that read as a finality check), which the
    /// assertions below redden on.
    #[test]
    fn a_signature_over_a_claim_the_network_never_made_is_accepted_and_the_verdict_says_so() {
        let identity = identity();
        let nonce = "cc".repeat(16);
        // Nothing produced this: not a block, not a state, not a height — a hash invented for the arm.
        let invented = BlockHash::new([0x5a; 32]);
        let sig = identity
            .attest_finality(&base16::decode(&nonce).unwrap(), &invented)
            .unwrap();
        let attestation = FinalityAttestation {
            block_hash: base16::encode(invented.as_bytes()),
            nonce: nonce.clone(),
            sender: base16::encode(identity.public_key.bytes()),
            sig_algorithm: identity.sig_algorithm.clone(),
            sig: base16::encode(&sig),
        };

        let verdict = verify_attestation(&serde_json::to_string(&attestation).unwrap(), &nonce)
            .expect("the signature is genuine, and a signature is all this tool can see");
        let parsed: serde_json::Value =
            serde_json::from_str(&verdict).expect("the verdict is JSON");

        assert_eq!(
            parsed["verified"],
            json!(true),
            "**the deviation stands: a signed claim about a block that never existed is accepted** \
             (W-N9-08) — the tool has no chain to check it against"
        );
        assert_eq!(
            parsed["blockHash"].as_str().unwrap(),
            base16::encode(invented.as_bytes()),
            "and it names the invented hash, because that is what was signed"
        );
        assert_eq!(
            parsed["proves"].as_str(),
            Some("this sender signed this block hash over this nonce"),
            "the verdict names the statement the signature covers, rather than claiming finality"
        );
        let limits: Vec<String> = parsed["doesNotProve"]
            .as_array()
            .expect("the verdict lists what it does not establish")
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect();
        for needle in ["finalised", "bonded", "quorum"] {
            assert!(
                limits.iter().any(|l| l.contains(needle)),
                "the limit on `{needle}` must be on the artefact an operator reads: {limits:?}"
            );
        }
    }
}
