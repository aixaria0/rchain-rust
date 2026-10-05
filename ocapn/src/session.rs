//! Session establishment: the `op:start-session` operation.
//!
//! `draft-specifications/CapTP Specification.md` fixes the operation's five fields and, for the
//! first two, the constants a sender MUST use. A session "is considered initialized once both
//! sides have sent and received `op:start-session`"; the Session ID both sides then agree on is
//! [`crate::session_id::session_id`], and a simultaneous exchange is settled by
//! [`crate::session_id::crossed_hello`].
//!
//! **A spec inconsistency carried, not resolved.** The construction section says `crypto-version`
//! MUST be `Ed25519_SHA256`; the *receiving* section says it MUST equal `Ed25519`. The document is
//! internally inconsistent and does not say which is correct, so this module sends the
//! construction constant and does not yet enforce a receive-side value. Deciding it is stage 0's
//! live-handshake question (the conformance suite), not a coin to flip here.

use std::fmt;

use crate::locator::PeerLocator;
use crate::syrup::Value;

/// `captp-version`, which "MUST be `1.0`".
pub const CAPTP_VERSION: &str = "1.0";
/// `crypto-version` on the sending side, which "MUST be `Ed25519_SHA256`".
pub const CRYPTO_VERSION: &str = "Ed25519_SHA256";
/// The record label that multiplexes this operation.
pub const START_SESSION_LABEL: &str = "op:start-session";
/// The record label that multiplexes `op:abort`.
pub const ABORT_LABEL: &str = "op:abort";

/// The `op:start-session` message — "carrying `captp-version`, `crypto-version`, `session-pubkey`,
/// `acceptable-location`, `acceptable-location-sig`".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSession {
    /// The per-session Ed25519 public key, serialized (32 bytes).
    pub session_pubkey: Vec<u8>,
    /// Where this peer accepts connections.
    pub acceptable_location: PeerLocator,
    /// The signature over the serialized location, under the session key.
    pub acceptable_location_sig: Vec<u8>,
}

impl StartSession {
    /// The `<op:start-session …>` record, fields in the spec's wire order.
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(START_SESSION_LABEL.to_string()),
            Value::String(CAPTP_VERSION.to_string()),
            Value::String(CRYPTO_VERSION.to_string()),
            Value::Bytes(self.session_pubkey.clone()),
            self.acceptable_location.to_syrup(),
            Value::Bytes(self.acceptable_location_sig.clone()),
        ])
    }

    /// Decode an `op:start-session` record, refusing a shape or version we do not speak.
    pub fn from_syrup(v: &Value) -> Result<StartSession, SessionError> {
        let Value::Record(fields) = v else {
            return Err(SessionError::NotAStartSession);
        };
        let [label, captp, crypto, pubkey, location, sig] = fields.as_slice() else {
            return Err(SessionError::NotAStartSession);
        };
        match label {
            Value::Symbol(s) if s == START_SESSION_LABEL => {}
            _ => return Err(SessionError::NotAStartSession),
        }
        let Value::String(captp) = captp else {
            return Err(SessionError::BadField("captp-version"));
        };
        if captp != CAPTP_VERSION {
            return Err(SessionError::UnsupportedVersion(captp.clone()));
        }
        // `crypto-version` is read but not yet enforced — see the module note on the spec's
        // self-contradiction (`Ed25519_SHA256` on send, `Ed25519` on receive).
        if !matches!(crypto, Value::String(_)) {
            return Err(SessionError::BadField("crypto-version"));
        }
        let Value::Bytes(pubkey) = pubkey else {
            return Err(SessionError::BadField("session-pubkey"));
        };
        let location = PeerLocator::from_syrup(location)
            .map_err(|_| SessionError::BadField("acceptable-location"))?;
        let Value::Bytes(sig) = sig else {
            return Err(SessionError::BadField("acceptable-location-sig"));
        };
        Ok(StartSession {
            session_pubkey: pubkey.clone(),
            acceptable_location: location,
            acceptable_location_sig: sig.clone(),
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
        match label {
            Value::Symbol(s) if s == ABORT_LABEL => {}
            _ => return Err(SessionError::NotAnAbort),
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
        StartSession {
            session_pubkey: vec![0u8; 32],
            acceptable_location: PeerLocator {
                designator: "abc".into(),
                transport: "tcp-testing-only".into(),
                hints: BTreeMap::new(),
            },
            acceptable_location_sig: vec![0xab, 0xcd],
        }
    }

    #[test]
    fn start_session_kat() {
        let mut expected = b"<16'op:start-session3\"1.014\"Ed25519_SHA25632:".to_vec();
        expected.extend_from_slice(&[0u8; 32]);
        expected.extend_from_slice(b"<10'ocapn-peer16'tcp-testing-only3\"abcf>");
        expected.extend_from_slice(b"2:");
        expected.extend_from_slice(&[0xab, 0xcd]);
        expected.push(b'>');
        assert_eq!(fixture().to_syrup().to_bytes(), expected);
    }

    #[test]
    fn start_session_round_trips_through_syrup_bytes() {
        let s = fixture();
        let bytes = s.to_syrup().to_bytes();
        let decoded = Value::from_bytes(&bytes).unwrap();
        assert_eq!(StartSession::from_syrup(&decoded).unwrap(), s);
    }

    #[test]
    fn start_session_refuses_an_unsupported_captp_version() {
        let mut fields = match fixture().to_syrup() {
            Value::Record(f) => f,
            _ => unreachable!(),
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
        // right label, wrong arity
        assert_eq!(
            StartSession::from_syrup(&Value::Record(vec![Value::Symbol(
                START_SESSION_LABEL.into()
            )])),
            Err(SessionError::NotAStartSession)
        );
    }

    #[test]
    fn start_session_refuses_a_wrong_typed_field() {
        let mut fields = match fixture().to_syrup() {
            Value::Record(f) => f,
            _ => unreachable!(),
        };
        fields[3] = Value::String("not-bytes".into()); // session-pubkey must be a ByteArray
        assert_eq!(
            StartSession::from_syrup(&Value::Record(fields)),
            Err(SessionError::BadField("session-pubkey"))
        );
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
