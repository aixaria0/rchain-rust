//! Third-party handoffs: the three records they turn on, and the signature over them.
//!
//! A handoff is how one peer gives another an object it did not create — the Gifter registers a gift
//! with the Exporter, tells the Receiver about it with a *give*, and the Receiver *withdraws* it from
//! the Exporter. Nothing in the exchange is a descriptor for an object the parties already share: the
//! object stays at the Exporter, and the Receiver ends up holding a reference to it there.
//!
//! The three records are the reference suite's (`utils/captp_types.py`), and the fixtures are the
//! specification here as everywhere in this crate:
//!
//! * `<desc:handoff-give receiver-key exporter-location session gifter-side gift-id>`
//! * `<desc:handoff-receive receiving-session receiving-side handoff-count signed-give>`
//! * `<desc:sig-envelope signed-object signature>`, where the signature is over the **syrup encoding
//!   of the signed object** and is carried in the gcrypt `(sig-val (eddsa (r …) (s …)))` shape — the
//!   same shape a start-session's location signature uses, so [`crate::session`]'s codec is reused
//!   rather than re-spelled.
//!
//! **The signature is the whole point of the receive.** Without it, anyone who observed a give could
//! withdraw the gift; with it, only the peer the give names (whose session key signs) can. The
//! Exporter verifies the receive against the give's own `receiver-key`, which is why that key travels
//! in the clear: it is the *name* of the party allowed to sign.

use rchain_crypto::signatures::ed25519::Ed25519;

use crate::locator::PeerLocator;
use crate::session::{public_key_bytes, public_key_syrup, signature_bytes, signature_syrup};
use crate::syrup::Value;

/// `<desc:handoff-give …>`
pub const HANDOFF_GIVE_LABEL: &str = "desc:handoff-give";
/// `<desc:handoff-receive …>`
pub const HANDOFF_RECEIVE_LABEL: &str = "desc:handoff-receive";
/// `<desc:sig-envelope …>`
pub const SIG_ENVELOPE_LABEL: &str = "desc:sig-envelope";

/// `<desc:handoff-give receiver-key exporter-location session gifter-side gift-id>`
///
/// The Gifter's statement: "the object deposited with the exporter as `gift-id` belongs to the peer
/// whose session key is `receiver-key`". `session` and `gifter-side` name *the gifter's own session
/// with the exporter*, so the receive can be checked against the connection it arrives on.
#[derive(Clone, Debug, PartialEq)]
pub struct HandoffGive {
    /// The receiver's session public key, as a gcrypt public-key record — the key the receive must
    /// be signed with.
    pub receiver_key: Value,
    /// Where the gift is deposited.
    pub exporter_location: PeerLocator,
    /// The gifter↔exporter session id.
    pub session: Vec<u8>,
    /// The gifter's public identifier on that session.
    pub gifter_side: Vec<u8>,
    /// The name the gift was deposited under.
    pub gift_id: Vec<u8>,
}

impl HandoffGive {
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(HANDOFF_GIVE_LABEL.to_string()),
            self.receiver_key.clone(),
            self.exporter_location.to_syrup(),
            Value::Bytes(self.session.clone()),
            Value::Bytes(self.gifter_side.clone()),
            Value::Bytes(self.gift_id.clone()),
        ])
    }

    /// The bytes a signature over this give covers: its own syrup encoding.
    pub fn signing_payload(&self) -> Vec<u8> {
        self.to_syrup().to_bytes()
    }

    pub fn from_syrup(v: &Value) -> Result<HandoffGive, String> {
        let Value::Record(fields) = v else {
            return Err("a handoff-give is a record".to_string());
        };
        let [label, receiver_key, exporter_location, session, gifter_side, gift_id] =
            fields.as_slice()
        else {
            return Err("a handoff-give has five fields".to_string());
        };
        if !matches!(label, Value::Symbol(s) if s == HANDOFF_GIVE_LABEL) {
            return Err(format!("not a {HANDOFF_GIVE_LABEL} record: {label:?}"));
        }
        let exporter_location = PeerLocator::from_syrup(exporter_location)
            .map_err(|e| format!("handoff-give exporter-location: {e}"))?;
        Ok(HandoffGive {
            receiver_key: receiver_key.clone(),
            exporter_location,
            session: bytes(session, "session")?,
            gifter_side: bytes(gifter_side, "gifter-side")?,
            gift_id: bytes(gift_id, "gift-id")?,
        })
    }
}

/// `<desc:sig-envelope signed-object signature>` — the object stays in its syrup form, because the
/// signature covers exactly those bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub object: Value,
    /// The gcrypt `(sig-val (eddsa (r …) (s …)))` record.
    pub signature: Value,
}

impl Envelope {
    /// Sign `object` with a session secret. The payload is the object's **syrup encoding**, which is
    /// what the reference verifies against.
    pub fn sign(object: Value, secret: &[u8; 32]) -> Result<Envelope, String> {
        let signature = Ed25519::sign_bytes(&object.to_bytes(), secret)
            .map_err(|e| format!("handoff signing: {e}"))?;
        Ok(Envelope {
            object,
            signature: signature_syrup(&signature).map_err(|e| e.to_string())?,
        })
    }

    /// Whether `signature` is `key_record`'s signature over `object`.
    ///
    /// The signature travels as the gcrypt record, so it is decoded first — `verify_bytes` takes the
    /// 64 raw bytes and the key as a record, the same pair of shapes a start-session's location
    /// signature uses.
    pub fn verifies(&self, key_record: &Value) -> bool {
        let (Ok(signature), Ok(key)) = (
            signature_bytes(&self.signature),
            public_key_bytes(key_record),
        ) else {
            return false;
        };
        Ed25519::verify_bytes(&self.object.to_bytes(), &signature, &key)
    }

    /// The signature's 64 bytes, for callers that compare rather than verify.
    pub fn signature_bytes(&self) -> Result<Vec<u8>, String> {
        signature_bytes(&self.signature).map_err(|e| e.to_string())
    }

    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(SIG_ENVELOPE_LABEL.to_string()),
            self.object.clone(),
            self.signature.clone(),
        ])
    }

    pub fn from_syrup(v: &Value) -> Result<Envelope, String> {
        let Value::Record(fields) = v else {
            return Err("a sig-envelope is a record".to_string());
        };
        let [label, object, signature] = fields.as_slice() else {
            return Err("a sig-envelope has two fields".to_string());
        };
        if !matches!(label, Value::Symbol(s) if s == SIG_ENVELOPE_LABEL) {
            return Err(format!("not a {SIG_ENVELOPE_LABEL} record: {label:?}"));
        }
        Ok(Envelope {
            object: object.clone(),
            signature: signature.clone(),
        })
    }
}

/// `<desc:handoff-receive receiving-session receiving-side handoff-count signed-give>`
///
/// The Receiver's claim on the gift — signed by the receiver, and verified by the exporter against
/// the give's `receiver-key`. `handoff-count` is the replay guard: one give may be withdrawn once per
/// count, so a replayed receive is refused.
#[derive(Clone, Debug, PartialEq)]
pub struct HandoffReceive {
    /// The receiver↔exporter session id, which the exporter checks against the connection.
    pub receiving_session: Vec<u8>,
    /// The receiver's public identifier on that session.
    pub receiving_side: Vec<u8>,
    pub handoff_count: u64,
    pub signed_give: Envelope,
}

impl HandoffReceive {
    pub fn to_syrup(&self) -> Value {
        Value::Record(vec![
            Value::Symbol(HANDOFF_RECEIVE_LABEL.to_string()),
            Value::Bytes(self.receiving_session.clone()),
            Value::Bytes(self.receiving_side.clone()),
            Value::Int(num_bigint::BigInt::from(self.handoff_count)),
            self.signed_give.to_syrup(),
        ])
    }

    pub fn from_syrup(v: &Value) -> Result<HandoffReceive, String> {
        let Value::Record(fields) = v else {
            return Err("a handoff-receive is a record".to_string());
        };
        let [label, receiving_session, receiving_side, handoff_count, signed_give] =
            fields.as_slice()
        else {
            return Err("a handoff-receive has four fields".to_string());
        };
        if !matches!(label, Value::Symbol(s) if s == HANDOFF_RECEIVE_LABEL) {
            return Err(format!("not a {HANDOFF_RECEIVE_LABEL} record: {label:?}"));
        }
        let count = match handoff_count {
            Value::Int(n) if n.sign() != num_bigint::Sign::Minus => {
                u64::try_from(n.magnitude().clone())
                    .map_err(|_| "handoff-count is larger than a u64".to_string())?
            }
            other => return Err(format!("handoff-count is not a count: {other:?}")),
        };
        Ok(HandoffReceive {
            receiving_session: bytes(receiving_session, "receiving-session")?,
            receiving_side: bytes(receiving_side, "receiving-side")?,
            handoff_count: count,
            signed_give: Envelope::from_syrup(signed_give)?,
        })
    }
}

/// The key record a handoff names: the peer's session public key, in the wire's gcrypt form.
pub fn key_record(key: &[u8]) -> Value {
    public_key_syrup(key)
}

/// What a deposited gift holds: where the object is on the *depositing* peer's connection, and the
/// handoff counts already withdrawn from it.
struct Gift {
    /// `Desc::Export(N)`, meaning "the object that peer exports at N" — a depositor's import names its
    /// own export, and this is how we address it back.
    to: crate::captp::Desc,
    /// **The replay guard.** A give may be withdrawn once per handoff count, so a peer that replays
    /// the same receive is refused rather than handed the object twice.
    withdrawn: std::collections::BTreeSet<u64>,
}

/// The **exporter** half of third-party handoffs, shared across every session this peer serves.
///
/// It has to be shared, and it has to be per peer rather than per session: a Gifter deposits a gift
/// on *its* connection and a Receiver withdraws it on *its own* — two different sessions of ours — so
/// a store inside one session's bootstrap would never see the other's deposit. That is exactly the
/// suite's `test_valid_handoff_wait_deposit_gift`, which withdraws before the deposit arrives.
///
/// **A gift is identified by `(gift id, the session the gifter named)`**, and not by the gift id
/// alone. The gift id is the *gifter's* name for one handoff, and nothing stops two independent
/// handoffs from using the same one — the conformance suite does exactly that, running several
/// handoff tests in one process against one peer, all with `b"my-gift"`. Keyed by gift id alone, a
/// fresh test's claim would be refused by the *previous* test's replay guard: measured, and the
/// reason `test_valid_handoff_wait_deposit_gift` broke once its withdraw stopped being handled after
/// its deposit. Adding the gifter's session separates independent handoffs while keeping what the
/// guard is for — the give names that session too, so a replay of one handoff still hits its own
/// bucket.
#[derive(Default)]
pub struct Handoffs {
    gifts: std::sync::Mutex<std::collections::BTreeMap<(Vec<u8>, Vec<u8>), Gift>>,
}

impl Handoffs {
    /// Record a gift from `session` (the gifter's session with us, which its give also names).
    ///
    /// Re-depositing replaces the object but **keeps the replay guard**: a gifter that re-deposits
    /// and then re-claims with a handoff count already used is refused, which is what
    /// `test_handoff_receive_invalid_handoff_count` asserts — and it asserts it *after* re-depositing,
    /// so a deposit that reset the guard would hand the object over a second time.
    pub fn deposit(&self, gift_id: Vec<u8>, session: Vec<u8>, to: crate::captp::Desc) {
        let mut gifts = self.gifts.lock().unwrap_or_else(|p| p.into_inner());
        match gifts.entry((gift_id, session)) {
            std::collections::btree_map::Entry::Occupied(mut entry) => entry.get_mut().to = to,
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Gift {
                    to,
                    withdrawn: std::collections::BTreeSet::new(),
                });
            }
        }
    }

    /// Whether a gift from `session` is deposited.
    pub fn has(&self, gift_id: &[u8], session: &[u8]) -> bool {
        self.gifts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&(gift_id.to_vec(), session.to_vec()))
    }

    /// Take a withdrawal: `Ok(Some(object))` when the gift is there and the count is fresh,
    /// `Ok(None)` when it is not deposited yet, `Err` when the count was already used.
    pub fn withdraw(
        &self,
        gift_id: &[u8],
        session: &[u8],
        count: u64,
    ) -> Result<Option<crate::captp::Desc>, String> {
        let mut gifts = self.gifts.lock().unwrap_or_else(|p| p.into_inner());
        let Some(gift) = gifts.get_mut(&(gift_id.to_vec(), session.to_vec())) else {
            return Ok(None);
        };
        if !gift.withdrawn.insert(count) {
            return Err(format!(
                "handoff count {count} has already been withdrawn for this gift"
            ));
        }
        Ok(Some(gift.to.clone()))
    }
}

fn bytes(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(format!("handoff {what} is not a byte array: {other:?}")),
    }
}
