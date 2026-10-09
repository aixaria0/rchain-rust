//! The bootstrap object: position 0 of every session.
//!
//! `CapTP Specification.md`: the bootstrap is "always the first export, at position 0", and its
//! methods are `fetch` (a swiss number for an object), `deposit-gift`, and `withdraw-gift`.
//!
//! * **`fetch`** resolves a swiss number to an object — the path the conformance suite uses to reach
//!   every fixture.
//! * **`deposit-gift`** and **`withdraw-gift`** are the Exporter's half of a third-party handoff: a
//!   Gifter deposits an object it holds, and a Receiver withdraws it, with a signed *receive* as the
//!   claim. The store they share lives in [`crate::handoff::Handoffs`] and is **per peer, not per
//!   session**, because the two halves arrive on two different connections of ours.
//!
//! The directory is deliberately a plain map from swiss number to object, with no capability
//! hiding: reachability is what a swiss number *is* here, and the objects it names are the ones the
//! node chose to publish.

use rchain_shared::lock::Unpoison;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rchain_shared::base16;

use crate::captp::Desc;
use crate::conn::{Act, Export, Reply};
use crate::handoff::{Envelope, HandoffGive, HandoffReceive, Handoffs};
use crate::owner::{SessionRegistry, SessionSlot};
use crate::proxy::Forward;
use crate::syrup::Value;

/// How long a withdrawal waits for its deposit to arrive.
///
/// The suite's `test_valid_handoff_wait_deposit_gift` withdraws *before* the deposit and expects the
/// withdrawal to resolve anyway, so the two halves genuinely race: the Receiver's claim can reach us
/// before the Gifter's gift does. Bounded, because a claim for a gift that never arrives has to fail
/// rather than hold a session's loop open.
const DEPOSIT_WAIT: Duration = Duration::from_secs(10);
const DEPOSIT_POLL: Duration = Duration::from_millis(10);

/// A directory of swiss numbers to the objects they name, as the bootstrap's `fetch` sees it.
pub struct Bootstrap {
    directory: BTreeMap<Vec<u8>, Arc<dyn Export>>,
    /// The exporter half of a handoff, shared with the peer's other sessions.
    handoffs: Arc<Handoffs>,
    /// This session, once it exists — a deposited object lives on the connection its gifter sent it
    /// on, so answering a withdrawal means naming *that* session.
    session: SessionSlot,
    /// The live sessions, so a withdrawal can find the connection a give names by its id.
    registry: Arc<SessionRegistry>,
}

impl Default for Bootstrap {
    fn default() -> Self {
        Bootstrap::new(BTreeMap::new())
    }
}

impl Bootstrap {
    pub fn new(directory: BTreeMap<Vec<u8>, Arc<dyn Export>>) -> Bootstrap {
        Bootstrap {
            directory,
            handoffs: Arc::new(Handoffs::default()),
            session: crate::owner::session_slot(),
            registry: Arc::new(SessionRegistry::default()),
        }
    }

    /// A bootstrap wired for handoffs: the shared gift store, its own session slot, and the registry
    /// that turns a give's session id back into a connection.
    pub fn with_handoffs(
        directory: BTreeMap<Vec<u8>, Arc<dyn Export>>,
        handoffs: Arc<Handoffs>,
        session: SessionSlot,
        registry: Arc<SessionRegistry>,
    ) -> Bootstrap {
        Bootstrap {
            directory,
            handoffs,
            session,
            registry,
        }
    }

    /// Publish an object under a swiss number.
    pub fn publish(&mut self, swiss_num: impl Into<Vec<u8>>, object: Arc<dyn Export>) -> &mut Self {
        self.directory.insert(swiss_num.into(), object);
        self
    }
}

#[async_trait]
impl Export for Bootstrap {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        match args.first() {
            Some(Value::Symbol(method)) if method == "fetch" => {
                // **The two implementations disagree about the swiss number's type.** The Locators
                // draft calls it a string and Endo sends one; the Python conformance suite sends a
                // byte array. A peer that speaks to both must accept both, so this does — the
                // directory is keyed by bytes either way, and a string is its UTF-8.
                let key: Vec<u8> = match args.get(1) {
                    Some(Value::Bytes(b)) => b.clone(),
                    Some(Value::String(s)) => s.as_bytes().to_vec(),
                    _ => {
                        return Err(
                            "fetch expects a swiss number, as bytes or as a string".to_string()
                        )
                    }
                };
                match self.directory.get(&key) {
                    Some(object) => Ok(Act::object(object.clone())),
                    None => Err(format!(
                        "no object at swiss number {}",
                        base16::encode(&key)
                    )),
                }
            }
            Some(Value::Symbol(m)) if m == "deposit-gift" => {
                let [_, gift_id, object] = args else {
                    return Err("deposit-gift expects a gift id and an object".to_string());
                };
                let gift_id = match gift_id {
                    Value::Bytes(b) => b.clone(),
                    other => return Err(format!("deposit-gift expects a byte gift id: {other:?}")),
                };
                // The object arrives as the *gifter's* descriptor — `desc:import-object N`, naming
                // the gifter's own export N — so we address it back the way `fulfil` does.
                let to = match Desc::from_syrup(object) {
                    Ok(Desc::ImportObject(n) | Desc::ImportPromise(n)) => Desc::Export(n),
                    Ok(other) => other,
                    Err(e) => return Err(format!("deposit-gift expects an object: {e}")),
                };
                // The gift belongs to the **gifter's session with us** — the session this delivery
                // arrived on, which is also the session the give will name — so that is half the key.
                let session = self
                    .session
                    .lock()
                    .unpoison()
                    .clone()
                    .and_then(|s| s.handle.id.as_ref().map(|id| id.to_vec()))
                    .ok_or_else(|| "a deposit arrived on a session with no id".to_string())?;
                // A refused deposit is a refused deposit: the peer is told why rather than being left
                // to discover it when its withdrawal times out (AUDIT C223).
                self.handoffs.deposit(gift_id, session, to)?;
                Ok(Act::nothing())
            }
            Some(Value::Symbol(m)) if m == "withdraw-gift" => {
                let Some(signed) = args.get(1) else {
                    return Err("withdraw-gift expects a signed handoff receive".to_string());
                };
                let claim = Envelope::from_syrup(signed)?;
                let receive = HandoffReceive::from_syrup(&claim.object)?;
                let give = HandoffGive::from_syrup(&receive.signed_give.object)?;

                // **The signature is what makes the claim a claim.** The give names the receiver's
                // session key; only a peer holding that key can produce a receive that verifies
                // against it, so an observer of the give cannot withdraw the gift.
                if !claim.verifies(&give.receiver_key) {
                    return Err(
                        "the handoff receive is not signed by the key the give names".to_string(),
                    );
                }
                // And the receive must be for *this* peer's session with us, named by id.
                let session = self
                    .session
                    .lock()
                    .unpoison()
                    .clone()
                    .ok_or_else(|| "the exporter has no session yet".to_string())?;
                if let Some(id) = session.handle.id.as_ref() {
                    if receive.receiving_session != id.to_vec() {
                        return Err(
                            "the handoff receive names another session than the one it arrived on"
                                .to_string(),
                        );
                    }
                } else {
                    return Err(
                        "the handoff receive arrived on a session with no completed handshake"
                            .to_string(),
                    );
                }

                // **The wait belongs to the loop, not to this delivery** (Law 61, AUDIT C223). The
                // gift may not have arrived yet — the two halves of a handoff race — and waiting for
                // it *here* stalled the whole session: `handle_deliver` is awaited on the loop task,
                // so nothing else on this session was read until the wait ended. A claim that is
                // early now returns a future instead, and the session goes on serving. The
                // observable is the *stall*, not the timeout.
                let handoffs = self.handoffs.clone();
                let registry = self.registry.clone();
                let gift_id = give.gift_id.clone();
                let session_id = give.session.clone();
                let count = receive.handoff_count;
                if let Some(to) = handoffs.withdraw(&gift_id, &session_id, count)? {
                    return forward_to(&registry, &session_id, to);
                }
                return Ok(Act {
                    out: Vec::new(),
                    reply: Reply::Deferred(Box::pin(async move {
                        let deadline = tokio::time::Instant::now() + DEPOSIT_WAIT;
                        loop {
                            if let Some(to) = handoffs.withdraw(&gift_id, &session_id, count)? {
                                return forward_to(&registry, &session_id, to);
                            }
                            if tokio::time::Instant::now() >= deadline {
                                return Err(format!(
                                    "no gift is deposited under {}",
                                    base16::encode(&gift_id)
                                ));
                            }
                            tokio::time::sleep(DEPOSIT_POLL).await;
                        }
                    })),
                });
            }
            other => Err(format!("unknown bootstrap method: {other:?}")),
        }
    }
}

/// Answer a withdrawal with a **forward** to the session the gift lives on.
///
/// The object lives on the **gifter's** session, which the give names by id. Answering the receiver
/// with a forward is what a handoff *is*: the receiver ends up holding something it never had a
/// connection with.
///
/// A free function rather than a method so both the immediate answer and the deferred one (Law 61)
/// use it — the deferred future cannot borrow the `Bootstrap`, since the loop owns the session by
/// then, so the two things it needs are passed in.
fn forward_to(
    registry: &SessionRegistry,
    session: &[u8],
    to: crate::captp::Desc,
) -> Result<Act, String> {
    let id = crate::session_id::Octets32::try_from(session)
        .map_err(|_| "the give's session id is not a session id".to_string())?;
    let Some(gifter) = registry.by_id(&id) else {
        return Err("the give names a session this peer does not have".to_string());
    };
    Ok(Act::object(Arc::new(Forward::new(gifter, to))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conn::Reply;

    const SWISS: &[u8] = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w";

    struct Marker;

    /// The public key a *secret* stands for, in the wire's record form. A seed is not a public key:
    /// verifying a signature against the secret's own bytes fails, which is what this helper keeps a
    /// test from spelling wrong (measured — a claim signed with `[7; 32]` does not verify against
    /// `public_key_syrup(&[7; 32])`, and the test then asserted the wrong refusal).
    fn key_record(secret: &[u8; 32]) -> Value {
        let public = rchain_crypto::signatures::ed25519::Ed25519::to_public_bytes(secret)
            .expect("a 32-byte secret has a public key");
        crate::session::public_key_syrup(&public)
    }

    #[async_trait]
    impl Export for Marker {
        async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
            Ok(Act::value(Value::String("found".to_string())))
        }
    }

    fn catalogue() -> Bootstrap {
        let mut directory: BTreeMap<Vec<u8>, Arc<dyn Export>> = BTreeMap::new();
        directory.insert(SWISS.to_vec(), Arc::new(Marker));
        Bootstrap::new(directory)
    }

    /// **The two reference implementations disagree on the swiss number's type** (AUDIT C217): the
    /// Locators draft and Endo send a *string*; the Python conformance suite sends a *byte array*.
    /// A peer that speaks to both must accept both, so this one does.
    #[tokio::test]
    async fn bootstrap_deliver_accepts_a_swiss_number_as_bytes_or_as_a_string() {
        let bootstrap = catalogue();
        for swiss in [
            Value::Bytes(SWISS.to_vec()),
            Value::String(String::from_utf8(SWISS.to_vec()).unwrap()),
        ] {
            let act = bootstrap
                .deliver(&[Value::Symbol("fetch".into()), swiss.clone()])
                .await
                .expect("both spellings resolve");
            assert!(
                matches!(act.reply, Reply::Object(_)),
                "{swiss:?} should have found the object"
            );
        }
    }

    #[tokio::test]
    async fn bootstrap_refuses_a_swiss_number_of_any_other_type() {
        let bootstrap = catalogue();
        // `Act` is not `Debug` (it holds a `dyn Export`), so read the error arm rather than
        // `unwrap_err`.
        let reason = match bootstrap
            .deliver(&[Value::Symbol("fetch".into()), Value::Int(7.into())])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "an int was accepted as a swiss number".to_string(),
        };
        assert_eq!(
            reason,
            "fetch expects a swiss number, as bytes or as a string"
        );
    }

    #[tokio::test]
    async fn bootstrap_breaks_on_an_unknown_swiss_number_rather_than_hanging() {
        let bootstrap = catalogue();
        let reason = match bootstrap
            .deliver(&[Value::Symbol("fetch".into()), Value::Bytes(vec![1, 2, 3])])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "an unknown swiss number was accepted".to_string(),
        };
        assert!(reason.starts_with("no object at swiss number"), "{reason}");
    }

    /// **A withdraw for a gift nobody deposited breaks**, rather than holding the session — and it
    /// waits first, because the deposit may simply be later than the claim.
    #[tokio::test]
    async fn a_withdrawal_for_an_undeposited_gift_breaks() {
        let bootstrap = catalogue();
        let give = crate::handoff::HandoffGive {
            receiver_key: key_record(&[7u8; 32]),
            exporter_location: crate::locator::PeerLocator {
                designator: "peer".to_string(),
                transport: "tcp-testing-only".to_string(),
                hints: BTreeMap::new(),
            },
            session: vec![1u8; 32],
            gifter_side: vec![2u8; 32],
            gift_id: b"nothing-deposited".to_vec(),
        };
        let envelope = Envelope::sign(give.to_syrup(), &[9u8; 32]).unwrap();
        let receive = crate::handoff::HandoffReceive {
            receiving_session: vec![3u8; 32],
            receiving_side: vec![4u8; 32],
            handoff_count: 0,
            signed_give: envelope,
        };
        // The claim verifies — it is signed by exactly the key the give names — so the refusal this
        // asserts is the *next* check: an exporter with no session of its own cannot be answering a
        // withdrawal at all, and says so rather than waiting for a gift that can never arrive.
        let claim = Envelope::sign(receive.to_syrup(), &[7u8; 32]).unwrap();
        let reason = match bootstrap
            .deliver(&[Value::Symbol("withdraw-gift".into()), claim.to_syrup()])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "an undeposited gift was withdrawn".to_string(),
        };
        assert_eq!(reason, "the exporter has no session yet");
    }

    /// A claim signed by a *different* key than the give names is refused — the check the whole
    /// record exists for. An observer of a give must not be able to withdraw the gift.
    #[tokio::test]
    async fn a_withdrawal_signed_by_another_key_is_refused() {
        let bootstrap = catalogue();
        let give = crate::handoff::HandoffGive {
            receiver_key: key_record(&[7u8; 32]),
            exporter_location: crate::locator::PeerLocator {
                designator: "peer".to_string(),
                transport: "tcp-testing-only".to_string(),
                hints: BTreeMap::new(),
            },
            session: vec![1u8; 32],
            gifter_side: vec![2u8; 32],
            gift_id: b"a-gift".to_vec(),
        };
        let envelope = Envelope::sign(give.to_syrup(), &[9u8; 32]).unwrap();
        let receive = crate::handoff::HandoffReceive {
            receiving_session: vec![3u8; 32],
            receiving_side: vec![4u8; 32],
            handoff_count: 0,
            signed_give: envelope,
        };
        // Signed with the wrong key: the give names [7; 32], this signs with [8; 32].
        let claim = Envelope::sign(receive.to_syrup(), &[8u8; 32]).unwrap();
        let reason = match bootstrap
            .deliver(&[Value::Symbol("withdraw-gift".into()), claim.to_syrup()])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "a forged claim was accepted".to_string(),
        };
        assert_eq!(
            reason,
            "the handoff receive is not signed by the key the give names"
        );
    }
}
