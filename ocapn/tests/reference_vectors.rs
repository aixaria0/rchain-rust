//! Differential vectors: this crate's wire encodings against the reference implementation.
//!
//! The OCapN prose and its reference implementation disagree often enough that the implementation
//! is the oracle (AUDIT C216). These vectors were produced by the reference's own encoder —
//! `contrib/syrup.py`'s `syrup_encode` from `github.com/ocapn/ocapn-test-suite` — over the real
//! message shapes, so a byte that drifts here is a peer that stops understanding us.
//!
//! To regenerate, from a checkout of the suite:
//!
//! ```python
//! from contrib.syrup import syrup_encode, Record, Symbol
//! hints = {"host": "127.0.0.1", "port": "22045"}
//! peer = Record(Symbol("ocapn-peer"), [Symbol("tcp-testing-only"), "abc", hints])
//! print(syrup_encode(hints).hex())
//! print(syrup_encode(Record(Symbol("op:deliver"),
//!     [Record(Symbol("desc:export"), [0]), [Symbol("fetch"), b"VMDDd1voKWarCe2GvgLbxbVFysNzRPzx"],
//!      False, False])).hex())
//! ```
//!
//! (The struct ordering is the load-bearing part: the reference sorts members by the *encoded* key,
//! and the session signature covers a record containing one.)

use std::collections::BTreeMap;

use rchain_ocapn::captp::{Deliver, Desc};
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::session::{my_location_payload, StartSession};
use rchain_ocapn::syrup::Value;
use rchain_shared::base16;

fn hints() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("host".to_string(), "127.0.0.1".to_string()),
        ("port".to_string(), "22045".to_string()),
    ])
}

fn peer() -> PeerLocator {
    PeerLocator {
        designator: "abc".into(),
        transport: "tcp-testing-only".into(),
        hints: hints(),
    }
}

#[test]
fn hints_struct_matches_the_reference() {
    let v = Value::Struct(
        hints()
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    );
    assert_eq!(
        base16::encode(&v.to_bytes()),
        "7b3422686f737439223132372e302e302e313422706f7274352232323034357d"
    );
}

#[test]
fn peer_record_matches_the_reference() {
    assert_eq!(
        base16::encode(&peer().to_syrup().to_bytes()),
        "3c3130276f6361706e2d706565723136277463702d74657374696e672d6f6e6c7933226162637b3422686f737439223132372e302e302e313422706f7274352232323034357d3e"
    );
}

#[test]
fn deliver_matches_the_reference() {
    let d = Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![
            Value::Symbol("fetch".into()),
            Value::Bytes(b"VMDDd1voKWarCe2GvgLbxbVFysNzRPzx".to_vec()),
        ],
        answer_pos: None,
        resolve_me_desc: None,
    };
    assert_eq!(
        base16::encode(&d.to_syrup().to_bytes()),
        "3c3130276f703a64656c697665723c313127646573633a6578706f7274302b3e5b3527666574636833323a564d44446431766f4b5761724365324776674c627862564679734e7a52507a785d66663e"
    );
}

#[test]
fn start_session_matches_the_reference() {
    let mut sig = vec![0xab; 32];
    sig.extend_from_slice(&[0xcd; 32]);
    let s = StartSession {
        captp_version: "1.0".into(),
        session_pubkey: vec![0u8; 32],
        acceptable_location: peer(),
        acceptable_location_sig: sig,
    };
    assert_eq!(
        base16::encode(&s.to_syrup().unwrap().to_bytes()),
        "3c3136276f703a73746172742d73657373696f6e3322312e305b3130277075626c69632d6b65795b33276563635b352763757276653727456432353531395d5b3527666c616773352765646473615d5b31277133323a00000000000000000000000000000000000000000000000000000000000000005d5d5d3c3130276f6361706e2d706565723136277463702d74657374696e672d6f6e6c7933226162637b3422686f737439223132372e302e302e313422706f7274352232323034357d3e5b37277369672d76616c5b352765646473615b31277233323aabababababababababababababababababababababababababababababababab5d5b31277333323acdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd5d5d5d3e"
    );
}

/// The payload the session key signs — the one place a struct-ordering mistake would silently make
/// every handshake fail.
#[test]
fn my_location_payload_matches_the_reference() {
    assert_eq!(
        base16::encode(&my_location_payload(&peer()).to_bytes()),
        "3c3131276d792d6c6f636174696f6e3c3130276f6361706e2d706565723136277463702d74657374696e672d6f6e6c7933226162637b3422686f737439223132372e302e302e313422706f7274352232323034357d3e3e"
    );
}

/// Every vector must also *decode* back to the value that produced it, so the decoder and encoder
/// are checked against each other and the reference at once.
#[test]
fn reference_vectors_round_trip() {
    for hex in [
        "7b3422686f737439223132372e302e302e313422706f7274352232323034357d",
        "3c3130276f6361706e2d706565723136277463702d74657374696e672d6f6e6c7933226162637b3422686f737439223132372e302e302e313422706f7274352232323034357d3e",
    ] {
        let bytes = base16::decode(hex).expect("valid hex");
        let decoded = Value::from_bytes(&bytes).expect("decodes");
        assert_eq!(base16::encode(&decoded.to_bytes()), hex);
    }
}
