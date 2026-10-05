//! The in-band (Syrup) descriptors of a peer and a sturdyref.
//!
//! Where [`crate::locator`] carries the *out-of-band* URI form, this carries the *in-band* one
//! that travels inside a CapTP session. `Locators.md` fixes both shapes:
//!
//! ```text
//! <ocapn-peer transport designator hints>          ; transport a symbol, designator a string,
//!                                                  ; hints a struct or `f`
//! <ocapn-sturdyref peer swiss-num>                 ; peer the record above, swiss-num a string
//! ```
//!
//! Round-tripping through [`Value`] must be lossless: the URI and the record are two encodings of
//! one locator, so a locator read from a URI and the same locator read from a record compare equal.

use std::collections::BTreeMap;
use std::fmt;

use crate::locator::{PeerLocator, Sturdyref};
use crate::syrup::Value;

/// The record label that multiplexes a peer descriptor.
pub const PEER_LABEL: &str = "ocapn-peer";
/// The record label that multiplexes a sturdyref descriptor.
pub const STURDYREF_LABEL: &str = "ocapn-sturdyref";

impl PeerLocator {
    /// The `<ocapn-peer …>` record. Hints are always a struct — the reference's `OCapNPeer` passes
    /// its `hints` dict even when empty, so an empty hint set is `{}`, not the grammar's optional
    /// `f`. (Emitting `f` would encode the same locator to different bytes, and the session
    /// signature covers a record containing this struct.)
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(PEER_LABEL.to_string()),
            Value::Symbol(self.transport.clone()),
            Value::String(self.designator.clone()),
            Value::Struct(
                self.hints
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ),
        ])
    }

    /// Decode a `<ocapn-peer …>` record. Any other shape is refused.
    pub fn from_syrup(v: &Value) -> Result<PeerLocator, PeerError> {
        let Value::Record(fields) = v else {
            return Err(PeerError::NotAPeerRecord);
        };
        let [label, transport, designator, hints] = fields.as_slice() else {
            return Err(PeerError::NotAPeerRecord);
        };
        match label {
            Value::Symbol(s) if s == PEER_LABEL => {}
            _ => return Err(PeerError::NotAPeerRecord),
        }
        let Value::Symbol(transport) = transport else {
            return Err(PeerError::BadField("transport"));
        };
        let Value::String(designator) = designator else {
            return Err(PeerError::BadField("designator"));
        };
        let hints = match hints {
            Value::Bool(false) => BTreeMap::new(),
            Value::Struct(m) => {
                let mut out = BTreeMap::new();
                for (k, val) in m {
                    let Value::String(val) = val else {
                        return Err(PeerError::BadField("hint value"));
                    };
                    out.insert(k.clone(), val.clone());
                }
                out
            }
            _ => return Err(PeerError::BadField("hints")),
        };
        // **The fields become keys.** A locator arrives in the peer's own `op:start-session` and its
        // designator/transport are what the registry is keyed by, so the byte bound is applied here —
        // where the value is parsed — rather than at each place one is stored (AUDIT C223). A count
        // cap cannot help against a key that is itself megabytes.
        crate::capacity::check_peer_sized(designator, transport, &hints)
            .map_err(|_| PeerError::FieldTooLong)?;
        Ok(PeerLocator {
            designator: designator.clone(),
            transport: transport.clone(),
            hints,
        })
    }
}

impl Sturdyref {
    /// The `<ocapn-sturdyref …>` record. The swiss number is a **byte array**, not a string: the
    /// reference passes `bytes` (`b"VMDDd1voKWarCe2GvgLbxbVFysNzRPzx"`) and Syrup encodes bytes as
    /// `:<n>`, where the Locators prose says "string". The implementation wins (AUDIT C216).
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(STURDYREF_LABEL.to_string()),
            self.peer.to_syrup(),
            Value::Bytes(self.swiss_num.clone()),
        ])
    }

    /// Decode a `<ocapn-sturdyref …>` record. Any other shape is refused.
    pub fn from_syrup(v: &Value) -> Result<Sturdyref, PeerError> {
        let Value::Record(fields) = v else {
            return Err(PeerError::NotASturdyrefRecord);
        };
        let [label, peer, swiss] = fields.as_slice() else {
            return Err(PeerError::NotASturdyrefRecord);
        };
        match label {
            Value::Symbol(s) if s == STURDYREF_LABEL => {}
            _ => return Err(PeerError::NotASturdyrefRecord),
        }
        let peer = PeerLocator::from_syrup(peer)?;
        let Value::Bytes(swiss) = swiss else {
            return Err(PeerError::BadField("swiss-num"));
        };
        Ok(Sturdyref {
            peer,
            swiss_num: swiss.clone(),
        })
    }
}

/// A descriptor that is not the shape its label promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerError {
    NotAPeerRecord,
    NotASturdyrefRecord,
    /// A field held the wrong Syrup type; names the field.
    BadField(&'static str),
    /// A field is longer than the bound its use as a key allows (AUDIT C223).
    FieldTooLong,
}

impl fmt::Display for PeerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerError::NotAPeerRecord => write!(f, "peer: not an `<{PEER_LABEL} …>` record"),
            PeerError::NotASturdyrefRecord => {
                write!(f, "peer: not an `<{STURDYREF_LABEL} …>` record")
            }
            PeerError::BadField(name) => write!(f, "peer: field {name:?} has the wrong type"),
            PeerError::FieldTooLong => {
                write!(f, "peer: a locator field is past its length bound")
            }
        }
    }
}

impl std::error::Error for PeerError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syrup::Value;

    #[test]
    fn peer_record_kat_without_hints() {
        let l = PeerLocator {
            designator: "abc".into(),
            transport: "tcp-testing-only".into(),
            hints: BTreeMap::new(),
        };
        // Hints are an (empty) struct, matching the reference's `OCapNPeer`, not the grammar's `f`.
        assert_eq!(
            l.to_syrup().to_bytes(),
            b"<10'ocapn-peer16'tcp-testing-only3\"abc{}>".to_vec()
        );
        assert_eq!(PeerLocator::from_syrup(&l.to_syrup()).unwrap(), l);
    }

    #[test]
    fn sturdyref_record_kat() {
        let s = Sturdyref {
            peer: PeerLocator {
                designator: "abc".into(),
                transport: "tcp-testing-only".into(),
                hints: BTreeMap::new(),
            },
            swiss_num: b"s1".to_vec(),
        };
        // The swiss number is a byte array (`2:s1`), not a string (`2"s1`).
        assert_eq!(
            s.to_syrup().to_bytes(),
            b"<15'ocapn-sturdyref<10'ocapn-peer16'tcp-testing-only3\"abc{}>2:s1>".to_vec()
        );
        assert_eq!(Sturdyref::from_syrup(&s.to_syrup()).unwrap(), s);
    }

    #[test]
    fn peer_record_round_trips_through_syrup_bytes() {
        let l = PeerLocator::parse_uri("ocapn://abc.tcp-testing-only?host=127.0.0.1&port=22045")
            .unwrap();
        let bytes = l.to_syrup().to_bytes();
        let decoded = Value::from_bytes(&bytes).unwrap();
        assert_eq!(PeerLocator::from_syrup(&decoded).unwrap(), l);
    }

    #[test]
    fn sturdyref_uri_and_record_agree() {
        let uri = "ocapn://abc.tcp-testing-only/s/JadQ0++RzsD4M+40uLxTWVaVqM10DcBJ";
        let from_uri = Sturdyref::parse_uri(uri).unwrap();
        let from_record = Sturdyref::from_syrup(&from_uri.to_syrup()).unwrap();
        assert_eq!(from_uri, from_record);
    }

    #[test]
    fn peer_record_refuses_the_wrong_shape() {
        assert_eq!(
            PeerLocator::from_syrup(&Value::List(vec![])),
            Err(PeerError::NotAPeerRecord)
        );
        // right label, wrong arity
        assert_eq!(
            PeerLocator::from_syrup(&Value::Record(vec![
                Value::Symbol(PEER_LABEL.into()),
                Value::Symbol("tcp".into()),
            ])),
            Err(PeerError::NotAPeerRecord)
        );
        assert_eq!(
            Sturdyref::from_syrup(&Value::Record(vec![Value::Symbol(STURDYREF_LABEL.into())])),
            Err(PeerError::NotASturdyrefRecord)
        );
    }

    #[test]
    fn peer_record_refuses_a_wrong_typed_field() {
        let bad = Value::Record(vec![
            Value::Symbol(PEER_LABEL.into()),
            Value::Symbol("tcp".into()),
            Value::Symbol("not-a-string".into()), // designator must be a String
            Value::Bool(false),
        ]);
        assert_eq!(
            PeerLocator::from_syrup(&bad),
            Err(PeerError::BadField("designator"))
        );
    }
}
