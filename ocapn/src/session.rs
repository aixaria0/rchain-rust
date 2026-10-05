//! Session establishment: the `op:start-session` operation.
//!
//! A session "is considered initialized once both sides have sent and received `op:start-session`".
//! The Session ID both sides then agree on is [`crate::session_id::session_id`], and a simultaneous
//! exchange is settled by [`crate::session_id::crossed_hello`].
//!
//! **The spec's field list is wrong, and this module follows the reference implementation.** The
//! CapTP draft defines `op:start-session` with five fields — `captp-version`, `crypto-version`,
//! `session-pubkey`, `acceptable-location`, `acceptable-location-sig` — and even contradicts itself
//! on the `crypto-version` value (`Ed25519_SHA256` when constructing, `Ed25519` when receiving).
//! The OCapN test suite's own `OpStartSession` carries **four** fields: `captp_version`,
//! `session_pubkey`, `location`, `location_sig` — there is no `crypto-version` on the wire at all.
//! Emitting the spec's five would fail the handshake against every existing implementation, so the
//! four-field form is the one built here. (Found by reading the reference implementation rather
//! than the prose; recorded as AUDIT C216.)
//!
//! **Two fields are gcrypt s-expressions, not raw bytes.** `session_pubkey` is
//! `['public-key ['ecc ['curve 'Ed25519] ['flags 'eddsa] ['q <raw 32 bytes>]]]` and the signature
//! is `['sig-val ['eddsa ['r <32 bytes>] ['s <32 bytes>]]]` — Syrup *lists*, because that is the
//! gcrypt shape they are named for. Both are reproduced exactly, and the signed payload is
//! `<my-location <the locator record>>`.

use std::fmt;

use crate::locator::PeerLocator;
use crate::syrup::Value;

/// `captp-version`, which "MUST be `1.0`".
pub const CAPTP_VERSION: &str = "1.0";
/// The record label that multiplexes this operation.
pub const START_SESSION_LABEL: &str = "op:start-session";
/// The record label that multiplexes `op:abort`.
pub const ABORT_LABEL: &str = "op:abort";

/// An Ed25519 public key in the gcrypt s-expression form the reference implementation reads:
/// `['public-key ['ecc ['curve 'Ed25519] ['flags 'eddsa] ['q <raw 32 bytes>]]]`.
pub fn public_key_syrup(raw_public_key: &[u8]) -> Value {
    Value::List(vec![
        Value::Symbol("public-key".to_string()),
        Value::List(vec![
            Value::Symbol("ecc".to_string()),
            Value::List(vec![
                Value::Symbol("curve".to_string()),
                Value::Symbol("Ed25519".to_string()),
            ]),
            Value::List(vec![
                Value::Symbol("flags".to_string()),
                Value::Symbol("eddsa".to_string()),
            ]),
            Value::List(vec![
                Value::Symbol("q".to_string()),
                Value::Bytes(raw_public_key.to_vec()),
            ]),
        ]),
    ])
}

/// Read the raw key back out of [`public_key_syrup`]'s shape.
pub fn public_key_bytes(v: &Value) -> Result<Vec<u8>, SessionError> {
    let bad = || SessionError::BadField("session-pubkey");
    let Value::List(outer) = v else {
        return Err(bad());
    };
    let [label, ecc] = outer.as_slice() else {
        return Err(bad());
    };
    if !is_symbol(label, "public-key") {
        return Err(bad());
    }
    let Value::List(ecc) = ecc else {
        return Err(bad());
    };
    let [ecc_label, curve, flags, q] = ecc.as_slice() else {
        return Err(bad());
    };
    if !is_symbol(ecc_label, "ecc")
        || !is_pair(curve, "curve", "Ed25519")
        || !is_pair(flags, "flags", "eddsa")
    {
        return Err(bad());
    }
    let Value::List(q) = q else {
        return Err(bad());
    };
    let [q_label, q_bytes] = q.as_slice() else {
        return Err(bad());
    };
    match (q_label, q_bytes) {
        (Value::Symbol(s), Value::Bytes(b)) if s == "q" => Ok(b.clone()),
        _ => Err(bad()),
    }
}

/// An Ed25519 signature in the gcrypt s-expression form: `['sig-val ['eddsa ['r …] ['s …]]]`, where
/// `r` and `s` are the two 32-byte halves of the 64-byte signature.
pub fn signature_syrup(signature: &[u8]) -> Result<Value, SessionError> {
    if signature.len() != 64 {
        return Err(SessionError::BadSignatureLength(signature.len()));
    }
    let (r, s) = signature.split_at(32);
    Ok(Value::List(vec![
        Value::Symbol("sig-val".to_string()),
        Value::List(vec![
            Value::Symbol("eddsa".to_string()),
            Value::List(vec![
                Value::Symbol("r".to_string()),
                Value::Bytes(r.to_vec()),
            ]),
            Value::List(vec![
                Value::Symbol("s".to_string()),
                Value::Bytes(s.to_vec()),
            ]),
        ]),
    ]))
}

/// Read the 64-byte signature back out of [`signature_syrup`]'s shape.
pub fn signature_bytes(v: &Value) -> Result<Vec<u8>, SessionError> {
    let bad = || SessionError::BadField("acceptable-location-sig");
    let Value::List(outer) = v else {
        return Err(bad());
    };
    let [label, eddsa] = outer.as_slice() else {
        return Err(bad());
    };
    if !is_symbol(label, "sig-val") {
        return Err(bad());
    }
    let Value::List(eddsa) = eddsa else {
        return Err(bad());
    };
    let [eddsa_label, r, s] = eddsa.as_slice() else {
        return Err(bad());
    };
    if !is_symbol(eddsa_label, "eddsa") {
        return Err(bad());
    }
    let r = named_bytes(r, "r").ok_or_else(bad)?;
    let s = named_bytes(s, "s").ok_or_else(bad)?;
    if r.len() != 32 || s.len() != 32 {
        return Err(SessionError::BadSignatureLength(r.len() + s.len()));
    }
    let mut out = r;
    out.extend_from_slice(&s);
    Ok(out)
}

/// What the session key signs: `<my-location <the locator record>>`, mirroring the reference
/// implementation's `Record(label=Symbol("my-location"), args=[self.location.to_syrup_record()])`.
pub fn my_location_payload(location: &PeerLocator) -> Value {
    Value::Record(vec![
        Value::Symbol("my-location".to_string()),
        location.to_syrup(),
    ])
}

fn is_symbol(v: &Value, name: &str) -> bool {
    matches!(v, Value::Symbol(s) if s == name)
}

fn is_pair(v: &Value, name: &str, value: &str) -> bool {
    matches!(v, Value::List(xs) if matches!(xs.as_slice(), [Value::Symbol(a), Value::Symbol(b)] if a == name && b == value))
}

fn named_bytes(v: &Value, name: &str) -> Option<Vec<u8>> {
    let Value::List(xs) = v else { return None };
    match xs.as_slice() {
        [Value::Symbol(s), Value::Bytes(b)] if s == name => Some(b.clone()),
        _ => None,
    }
}

/// The `op:start-session` message, in the reference implementation's four-field form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSession {
    /// The version this peer speaks; `"1.0"`.
    pub captp_version: String,
    /// The per-session Ed25519 public key, raw (32 bytes); encoded via [`public_key_syrup`].
    pub session_pubkey: Vec<u8>,
    /// Where this peer accepts connections.
    pub acceptable_location: PeerLocator,
    /// The signature over [`my_location_payload`], raw (64 bytes); encoded via [`signature_syrup`].
    pub acceptable_location_sig: Vec<u8>,
}

impl StartSession {
    /// The `<op:start-session …>` record. Fails only on a signature that is not 64 bytes.
    pub fn to_syrup(&self) -> Result<Value, SessionError> {
        Ok(Value::Record(vec![
            Value::Symbol(START_SESSION_LABEL.to_string()),
            Value::String(self.captp_version.clone()),
            public_key_syrup(&self.session_pubkey),
            self.acceptable_location.to_syrup(),
            signature_syrup(&self.acceptable_location_sig)?,
        ]))
    }

    /// Decode an `op:start-session` record, refusing a shape or version we do not speak.
    pub fn from_syrup(v: &Value) -> Result<StartSession, SessionError> {
        let Value::Record(fields) = v else {
            return Err(SessionError::NotAStartSession);
        };
        let [label, version, pubkey, location, sig] = fields.as_slice() else {
            return Err(SessionError::NotAStartSession);
        };
        if !is_symbol(label, START_SESSION_LABEL) {
            return Err(SessionError::NotAStartSession);
        }
        let Value::String(version) = version else {
            return Err(SessionError::BadField("captp-version"));
        };
        if version != CAPTP_VERSION {
            return Err(SessionError::UnsupportedVersion(version.clone()));
        }
        let session_pubkey = public_key_bytes(pubkey)?;
        let acceptable_location = PeerLocator::from_syrup(location)
            .map_err(|_| SessionError::BadField("acceptable-location"))?;
        let acceptable_location_sig = signature_bytes(sig)?;
        Ok(StartSession {
            captp_version: version.clone(),
            session_pubkey,
            acceptable_location,
            acceptable_location_sig,
        })
    }
}

/// `op:abort` — "Ends the session, severing the connection and breaking unresolved promises." The
/// reason text is the peer's to choose; a peer must accept any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abort {
    pub reason: String,
}

impl Abort {
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(ABORT_LABEL.to_string()),
            Value::String(self.reason.clone()),
        ])
    }

    pub fn from_syrup(v: &Value) -> Result<Abort, SessionError> {
        let Value::Record(fields) = v else {
            return Err(SessionError::NotAnAbort);
        };
        let [label, reason] = fields.as_slice() else {
            return Err(SessionError::NotAnAbort);
        };
        if !is_symbol(label, ABORT_LABEL) {
            return Err(SessionError::NotAnAbort);
        }
        let Value::String(reason) = reason else {
            return Err(SessionError::BadField("reason"));
        };
        Ok(Abort {
            reason: reason.clone(),
        })
    }
}

/// An `op:start-session` that will not be accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotAStartSession,
    NotAnAbort,
    /// A field held the wrong Syrup type; names the field.
    BadField(&'static str),
    /// A session signature was not the 64 bytes an Ed25519 signature is.
    BadSignatureLength(usize),
    /// The peer speaks a `captp-version` this implementation does not.
    UnsupportedVersion(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotAStartSession => {
                write!(f, "session: not an `<{START_SESSION_LABEL} …>` record")
            }
            SessionError::NotAnAbort => write!(f, "session: not an `<{ABORT_LABEL} …>` record"),
            SessionError::BadField(name) => write!(f, "session: field {name:?} has the wrong type"),
            SessionError::BadSignatureLength(n) => {
                write!(
                    f,
                    "session: signature is {n} bytes, not the 64 an Ed25519 signature is"
                )
            }
            SessionError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "session: unsupported captp-version {v:?} (we speak {CAPTP_VERSION:?})"
                )
            }
        }
    }
}

impl std::error::Error for SessionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> StartSession {
        let mut sig = vec![0xab; 32];
        sig.extend_from_slice(&[0xcd; 32]);
        StartSession {
            captp_version: CAPTP_VERSION.to_string(),
            session_pubkey: vec![0u8; 32],
            acceptable_location: PeerLocator {
                designator: "abc".into(),
                transport: "tcp-testing-only".into(),
                hints: BTreeMap::new(),
            },
            acceptable_location_sig: sig,
        }
    }

    #[test]
    fn public_key_list_kat() {
        let mut expected =
            b"[10'public-key[3'ecc[5'curve7'Ed25519][5'flags5'eddsa][1'q32:".to_vec();
        expected.extend_from_slice(&[0u8; 32]);
        expected.extend_from_slice(b"]]]");
        assert_eq!(public_key_syrup(&[0u8; 32]).to_bytes(), expected);
        assert_eq!(
            public_key_bytes(&public_key_syrup(&[0u8; 32])).unwrap(),
            vec![0u8; 32]
        );
    }

    #[test]
    fn signature_list_kat() {
        let mut sig = vec![0xab; 32];
        sig.extend_from_slice(&[0xcd; 32]);
        let mut expected = b"[7'sig-val[5'eddsa[1'r32:".to_vec();
        expected.extend_from_slice(&[0xab; 32]);
        expected.extend_from_slice(b"][1's32:");
        expected.extend_from_slice(&[0xcd; 32]);
        expected.extend_from_slice(b"]]]");
        assert_eq!(signature_syrup(&sig).unwrap().to_bytes(), expected);
        assert_eq!(
            signature_bytes(&signature_syrup(&sig).unwrap()).unwrap(),
            sig
        );
    }

    #[test]
    fn start_session_has_the_reference_four_fields() {
        let mut sig = vec![0xab; 32];
        sig.extend_from_slice(&[0xcd; 32]);
        let expected = Value::Record(vec![
            Value::Symbol("op:start-session".into()),
            Value::String("1.0".into()),
            Value::List(vec![
                Value::Symbol("public-key".into()),
                Value::List(vec![
                    Value::Symbol("ecc".into()),
                    Value::List(vec![
                        Value::Symbol("curve".into()),
                        Value::Symbol("Ed25519".into()),
                    ]),
                    Value::List(vec![
                        Value::Symbol("flags".into()),
                        Value::Symbol("eddsa".into()),
                    ]),
                    Value::List(vec![Value::Symbol("q".into()), Value::Bytes(vec![0u8; 32])]),
                ]),
            ]),
            Value::Record(vec![
                Value::Symbol("ocapn-peer".into()),
                Value::Symbol("tcp-testing-only".into()),
                Value::String("abc".into()),
                Value::Bool(false),
            ]),
            Value::List(vec![
                Value::Symbol("sig-val".into()),
                Value::List(vec![
                    Value::Symbol("eddsa".into()),
                    Value::List(vec![
                        Value::Symbol("r".into()),
                        Value::Bytes(vec![0xab; 32]),
                    ]),
                    Value::List(vec![
                        Value::Symbol("s".into()),
                        Value::Bytes(vec![0xcd; 32]),
                    ]),
                ]),
            ]),
        ]);
        assert_eq!(fixture().to_syrup().unwrap(), expected);
        // The regression this test exists for: the label plus *four* fields, never the spec's five.
        let Value::Record(fields) = expected else {
            unreachable!()
        };
        assert_eq!(fields.len(), 5);
    }

    #[test]
    fn start_session_round_trips_through_syrup_bytes() {
        let s = fixture();
        let bytes = s.to_syrup().unwrap().to_bytes();
        let decoded = Value::from_bytes(&bytes).unwrap();
        assert_eq!(StartSession::from_syrup(&decoded).unwrap(), s);
    }

    #[test]
    fn start_session_refuses_an_unsupported_captp_version() {
        let Value::Record(mut fields) = fixture().to_syrup().unwrap() else {
            unreachable!()
        };
        fields[1] = Value::String("2.0".into());
        assert_eq!(
            StartSession::from_syrup(&Value::Record(fields)),
            Err(SessionError::UnsupportedVersion("2.0".into()))
        );
    }

    #[test]
    fn start_session_refuses_the_wrong_shape() {
        assert_eq!(
            StartSession::from_syrup(&Value::List(vec![])),
            Err(SessionError::NotAStartSession)
        );
        // The spec's five-field form — the one this module deliberately does not speak.
        let Value::Record(mut five) = fixture().to_syrup().unwrap() else {
            unreachable!()
        };
        five.insert(2, Value::String("Ed25519_SHA256".into()));
        assert_eq!(
            StartSession::from_syrup(&Value::Record(five)),
            Err(SessionError::NotAStartSession)
        );
    }

    #[test]
    fn signature_syrup_refuses_a_wrong_length() {
        assert_eq!(
            signature_syrup(&[0u8; 63]),
            Err(SessionError::BadSignatureLength(63))
        );
        assert!(fixture().to_syrup().is_ok());
    }

    #[test]
    fn abort_kat_and_round_trip() {
        let a = Abort {
            reason: "bye".into(),
        };
        assert_eq!(a.to_syrup().to_bytes(), b"<8'op:abort3\"bye>".to_vec());
        assert_eq!(Abort::from_syrup(&a.to_syrup()).unwrap(), a);
    }

    #[test]
    fn abort_refuses_the_wrong_shape() {
        assert_eq!(
            Abort::from_syrup(&Value::List(vec![])),
            Err(SessionError::NotAnAbort)
        );
        assert_eq!(
            Abort::from_syrup(&Value::Record(vec![Value::Symbol("op:deliver".into())])),
            Err(SessionError::NotAnAbort)
        );
        assert_eq!(
            Abort::from_syrup(&Value::Record(vec![
                Value::Symbol(ABORT_LABEL.into()),
                Value::Bool(true),
            ])),
            Err(SessionError::BadField("reason"))
        );
    }
}
