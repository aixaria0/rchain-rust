//! Owning a session: a handle to send through, the loop that owns it, and the rule that says which
//! of two simultaneous sessions between the same peers dies.
//!
//! **Why this exists.** A session's tables and socket are single-owner ([`crate::conn`]'s loop), and
//! an object on a session sends by returning an `Act` the loop performs. That is enough for a peer
//! that only *answers*, and it is what the first three stages were built for. It is not enough for a
//! sturdyref enlivener, which has to open a **second** session — dial the peer a sturdyref names and
//! fetch an object from it — while it is inside a delivery on the first. So:
//!
//! * [`SessionHandle`] is what a task that owns a session gives out: enough to *ask* the loop to
//!   send, plus the session's identifiers (which a handoff has to name);
//! * [`SessionLoop`] is the task that owns the socket and the tables. Nothing else touches them, so
//!   there is still no lock around either socket and no deadlock when one session talks to another.
//!
//! **The crossed-hello rule**, which the suite tests from both sides: when two peers have dialed each
//! other, exactly one of the two sessions survives, and both peers agree which. The spec's sentence
//! is "the lower of the two has its connection aborted"; the pair it compares is the **two dialing
//! sides** — the public identifier of the peer we dialed, against the public identifier the peer used
//! on the connection it dialed. (Not each connection's own key pair: the two connections have four
//! keys between them, and only the two dialing ones are compared by both sides alike.) So the session
//! *dialed by the side whose dialing identifier is lower* is the one that aborts — which is what
//! `op_start_session.py` asserts in each variant, one on the leg the implementation dialed and one on
//! the leg it accepted.

use rchain_shared::chan;
use rchain_shared::lock::Unpoison;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::captp::Deliver;
use crate::captp::Desc;
use crate::conn::{Act, ConnectionError, Export, Session};
use crate::locator::PeerLocator;
use crate::netlayer::NetConn;
use crate::session_id::{crossed_hello, CrossedHello, Octets32};
use crate::syrup::Value;

/// How many hand-offs may be queued for a session before a sender waits. Bounded, because every
/// queue on a path a peer can drive has to be (`tools/check-bounded-ingress-queues.sh`).
const HANDOFF_DEPTH: usize = 64;

/// A message from a handle to the loop that owns the session: send a delivery, and let the answer
/// (or the break) land on `resolve_me`.
pub struct HandOff {
    pub to: Desc,
    pub args: Vec<Value>,
    /// Exported on *that* session and offered as the delivery's `resolve-me-desc`.
    pub resolve_me: Option<Arc<dyn Export>>,
}

/// What a task that owns a session hands out.
#[derive(Clone)]
pub struct SessionHandle {
    tx: mpsc::Sender<HandOff>,
    abort: mpsc::Sender<()>,
    /// The shared session id — `None` until the peer's start-session arrives.
    pub id: Option<Octets32>,
    /// Our public identifier on this session, and the peer's once it has answered. The crossed-hello
    /// rule compares *our dialing identifier* on both legs, so ours is the one that must be here.
    pub own_pi: Octets32,
    pub peer_pi: Option<Octets32>,
    /// The peer's session public-key bytes, as sent. A handoff's `receiver_key` names these.
    pub peer_key: Option<Vec<u8>>,
    /// True when we dialed this session, false when we accepted it.
    pub dialed: bool,
    /// **Where the peer is**, when the transport knows (Law 62, AUDIT C225). A dial this peer asks
    /// for is judged against it: a remote peer may not make this node reach the node's own loopback.
    pub peer_address: Option<std::net::SocketAddr>,
}

/// How long a sender waits for room in a session's hand-off queue before giving up.
///
/// **The queue is bounded and the *wait* was not.** Every receive in this crate carries a bound (the
/// handshake, the enliven fetch, the forward — all 30 s), and the send did not: a peer that kept a
/// target session's queue full (depth [`HANDOFF_DEPTH`], fed by cross-session forwards) could block a
/// *different* task's send for ever — and that task is holding *its own* session's loop, which then
/// cannot drain. Bounding the send turns a permanent wedge into a stalled delivery that reports.
pub const HANDOFF_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl SessionHandle {
    /// Send a delivery on this session. `Err` when the session has ended, or when its loop did not
    /// take the delivery within [`HANDOFF_SEND_TIMEOUT`].
    pub async fn deliver(&self, hand_off: HandOff) -> Result<(), ConnectionError> {
        match tokio::time::timeout(HANDOFF_SEND_TIMEOUT, self.tx.send(hand_off)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(ConnectionError::Closed),
            Err(_) => Err(ConnectionError::Protocol(
                "the session's hand-off queue stayed full".to_string(),
            )),
        }
    }

    /// Ask the loop to abort this session (the crossed-hello rule uses this on the loser).
    pub async fn abort(&self) {
        chan::try_send(&self.abort, ());
    }

    /// The abort signal itself, for the registry to keep: an older session that loses a crossing has
    /// to be stopped by whoever notices the crossing, and that is not its own owner.
    pub fn abort_sender(&self) -> mpsc::Sender<()> {
        self.abort.clone()
    }
}

/// The task that owns a session.
pub struct SessionLoop {
    session: Session,
    rx: mpsc::Receiver<HandOff>,
    abort: mpsc::Receiver<()>,
    /// **Answers that landed after the delivery that owed them** (Law 61). An object that cannot
    /// answer without waiting returns `Reply::Deferred` and a waiter sends the resolved act here, so
    /// the *loop* writes it and the session goes on serving meanwhile. AUDIT C223's stall was the
    /// alternative: the wait happened inside `handle_deliver`, and an unrelated delivery on the same
    /// session waited with it.
    deferred: mpsc::Receiver<(Deliver, Result<Act, String>)>,
}

impl SessionLoop {
    /// Write the `op:start-session` a deferred accept held back. Called once the session is booked.
    pub async fn announce(&mut self) -> Result<(), ConnectionError> {
        self.session.announce().await
    }

    /// Run until the peer aborts, the socket closes, or the crossed-hello rule kills this session.
    pub async fn run(mut self) -> Result<(), ConnectionError> {
        /// What one turn of the loop was woken by.
        enum Next {
            Abort,
            HandOff(Option<HandOff>),
            Answer(Option<(Deliver, Result<Act, String>)>),
            Message(Result<Option<Value>, ConnectionError>),
            NoHandles,
        }
        // Every handle can go away — the node's accepted sessions have none at all — and the session
        // still serves deliveries. `select!`'s `if` disables the branch instead of polling a channel
        // that is closed, which would spin.
        let mut handles_live = true;
        loop {
            // **The futures are built before the `select!`**, because `select!`'s bodies run while
            // the losing futures are still alive: `recv_message` holds a mutable borrow of the session
            // for the whole select, so the branch that writes a deferred answer could not otherwise
            // touch it. Pinning them here drops those borrows before the body runs.
            // The whole select lives in its own block: the pinned futures borrow the session, so they
            // must be dropped before the bodies below touch it again.
            let next = {
                let message = self.session.recv_message();
                let answer = self.deferred.recv();
                tokio::pin!(message, answer);
                tokio::select! {
                // The rule's word first: a session marked as the loser must not keep serving.
                    _ = self.abort.recv() => Next::Abort,
                    out = self.rx.recv(), if handles_live => match out {
                        Some(hand_off) => Next::HandOff(Some(hand_off)),
                        None => Next::NoHandles,
                    },
                    landed = &mut answer => Next::Answer(landed),
                    read = &mut message => Next::Message(read),
                }
            };
            match next {
                Next::Abort => {
                    self.session.abort_with("Crossed hellos mitigated").await;
                    return Ok(());
                }
                Next::NoHandles => handles_live = false,
                Next::HandOff(Some(hand_off)) => {
                    self.session
                        .hand_off(hand_off.to, hand_off.args, hand_off.resolve_me)
                        .await?;
                }
                Next::HandOff(None) => {}
                Next::Answer(Some((deliver, Ok(act)))) => {
                    // The answer the object could not give yet. Writing it here rather than on the
                    // waiter's task is what keeps the connection single-owner.
                    let _ = self.session.answer(&deliver, act).await?;
                }
                Next::Answer(Some((deliver, Err(reason)))) => {
                    self.session.break_answer(&deliver, reason).await?;
                }
                // Unreachable while the loop owns the session: the session's own sender keeps the
                // channel open, so a closed receiver would mean the session was dropped.
                Next::Answer(None) => {
                    return Err(ConnectionError::Protocol(
                        "the deferred-answer channel closed under the loop".to_string(),
                    ))
                }
                Next::Message(read) => {
                    let Some(message) = read? else { return Ok(()) };
                    if self.session.handle_message(message).await? == crate::conn::Flow::Stop {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Split a session into the handle a task may keep, the loop that must be run, and the context a
/// per-session object needs.
///
/// The context carries the session **secret**, which is why it is produced here rather than by the
/// caller: a handoff receive is signed with it, and only something that owns a session should be able
/// to sign as it.
pub(crate) fn split(mut session: Session) -> (SessionHandle, SessionLoop, SessionContext) {
    let (tx, rx) = mpsc::channel(HANDOFF_DEPTH);
    let (abort_tx, abort_rx) = mpsc::channel(1);
    // Taken from the session here because the loop must own it: only the loop writes to the
    // connection, and a deferred answer is an answer (Law 61).
    let deferred = session
        .take_deferred()
        .expect("a session's deferred-answer receiver is taken exactly once, by `split`");
    let (own_pi, peer_pi) = session.public_identifiers();
    let handle = SessionHandle {
        tx,
        abort: abort_tx,
        id: session.id.clone(),
        own_pi: own_pi.clone(),
        peer_pi: peer_pi.cloned(),
        peer_key: session.peer_key().map(<[u8]>::to_vec),
        dialed: session.is_dialed(),
        peer_address: session.peer_address(),
    };
    let context = SessionContext {
        handle: handle.clone(),
        secret: session.secret(),
    };
    (
        handle,
        SessionLoop {
            session,
            rx,
            abort: abort_rx,
            deferred,
        },
        context,
    )
}

/// Accept a connection, **book it, then answer it** — the order a peer's own use of the session
/// depends on, and the only order that keeps a handoff receiver from dialing a second session to a
/// peer it already has one with (see [`Session::accept_deferred`] and [`SessionLoop::announce`]).
///
/// Returns the serving parts of a session that won its crossing. `errors` are the two ways there is
/// nothing to serve: the handshake itself was refused (`Session::accept_deferred`'s own error, the
/// abort already written by that call), or this session was the crossing's loser — in which case the
/// abort has been written and the loop run once for it, because **the abort is a message on the
/// socket and the loop is what owns the socket**.
///
/// The locator it returns is the one the session was **registered under** — the key
/// [`SessionRegistry::forget`] needs, which is the peer's own advertised location whenever the peer is
/// who it says it is. Callers that only want to dial the peer back can use it for that too; see the
/// note on `book_key` below for the case where the two come apart.
pub async fn accept_and_book(
    conn: Box<dyn NetConn>,
    identity: &crate::conn::Identity,
    bootstrap: Arc<dyn Export>,
    registry: &SessionRegistry,
) -> Result<(SessionHandle, SessionLoop, SessionContext, PeerLocator), String> {
    let session = crate::conn::Session::accept_deferred(conn, identity, bootstrap)
        .await
        .map_err(|e| e.to_string())?;
    // An accepted session has read the peer's start-session before it exists, so its advertised
    // location is always here — and it is what a crossing with this peer would come from.
    let peer = session
        .peer()
        .map(|p| p.acceptable_location.clone())
        .ok_or_else(|| "an accepted session arrived with no peer start-session".to_string())?;
    // **A session is booked under the name its transport *proved*, when it proved one** (HAZOP rows
    // C242/C243). The peer's `acceptable-location` designator is a self-assertion: it is public — it
    // is the cleartext prefix of every Noise SYN and it is advertised in every location the node hands
    // out — and its signature covers only the peer's own ephemeral session key. Booking under it lets
    // a stranger who knows an honest peer's name collide with that peer's session and, by the crossing
    // rule, have it aborted. A transport that verified a key gives the session a name the peer cannot
    // forge; `noise` does and `websocket` cannot (its handshake has only the *server* prove itself),
    // so the two are treated differently on purpose rather than assumed equal.
    //
    // **The name is written into the `verify` hint, which is the field [`peer_key`] reads**, rather
    // than replacing the designator. The two agree for every peer that has an identity — a node's
    // OCapN designator *is* its verifying key in base16 — so this changes no honest peer's name. What
    // it changes is the liar's: the designator it asserted is not the name it is registered under,
    // and it cannot collide with the peer whose name it used. The peer's own hints are left alone,
    // because they are how it is dialled back.
    let book_key = match session.verified_peer_key() {
        Some(key) => {
            let mut booked = peer.clone();
            booked
                .hints
                .insert(VERIFY_HINT.to_string(), rchain_shared::base16::encode(&key));
            booked
        }
        None => peer.clone(),
    };
    let (handle, mut loop_, context) = session.split();
    match registry.admit(&book_key, &handle) {
        Ok(losers) => {
            for loser in losers {
                loser.abort().await;
            }
        }
        Err(reason) => {
            handle.abort().await;
            let _ = loop_.run().await;
            registry.forget(&book_key, &handle.own_pi, handle.dialed);
            return Err(reason);
        }
    }
    // Booked: the peer may now speak, so it may now be answered.
    if let Err(e) = loop_.announce().await {
        registry.forget(&book_key, &handle.own_pi, handle.dialed);
        return Err(e.to_string());
    }
    Ok((handle, loop_, context, book_key))
}

/// What the registry knows about one peer it has sessions with.
#[derive(Default)]
struct PeerSessions {
    /// The session we dialed: our dialing identifier, and the handle that can stop and use it.
    dialed: Option<(Octets32, SessionHandle)>,
    /// The session we accepted: the identifier the *peer* dialed with, and its handle.
    accepted: Option<(Octets32, SessionHandle)>,
}

/// The live sessions this peer has with others, so a second connection between the same two peers
/// can be resolved by the crossed-hello rule.
pub struct SessionRegistry {
    /// Keyed by *peer* — see [`peer_key`]: the designator and transport `PeerLocator::same_peer`
    /// compares, except where a transport's handshake supplies a key the peer had to prove, which
    /// takes the designator's place. Two locators that differ only in the *other* hints are the same
    /// peer, and a crossing between them is a crossing.
    ///
    /// **Bounded** (AUDIT C223), and bounded twice over: the key is the peer's own designator, so
    /// [`check_peer_sized`](crate::capacity::check_peer_sized) refuses a locator whose fields are past
    /// their byte bounds at *parse* time, and this cap holds the count. `forget` removes what a
    /// well-behaved peer leaves behind; the cap is what a peer varying its designator hits.
    peers: Mutex<crate::capacity::Bounded<(String, String), PeerSessions>>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        SessionRegistry {
            peers: Mutex::new(crate::capacity::Bounded::new(crate::capacity::MAX_PEERS)),
        }
    }
}

impl SessionRegistry {
    /// Record a completed handshake, and decide the crossing if there is one.
    ///
    /// `Ok(losers)` means this session was admitted and the sessions in `losers` must be stopped
    /// (an older session that just lost); `Err(reason)` means **this** session is the loser and its
    /// owner must send `op:abort` and drop it — which is what the suite asserts from both sides.
    pub fn admit(
        &self,
        peer: &PeerLocator,
        handle: &SessionHandle,
    ) -> Result<Vec<SessionHandle>, String> {
        let mut peers = self.peers.lock().unpoison();
        let key = peer_key(peer);
        if !peers.contains_key(&key) {
            // **A new peer past the cap is refused, not queued.** The map is keyed by the peer's own
            // designator, so without this a peer that varies it accumulates entries for ever
            // (AUDIT C223); the refusal is a handshake the caller answers with `op:abort`.
            peers
                .try_insert(key.clone(), PeerSessions::default())
                .map_err(|full| {
                    format!("this node is already tracking as many peers as it may ({full})")
                })?;
        }
        let Some(entry) = peers.get_mut(&key) else {
            // Not reachable: the key was just ensured present, and nothing else holds this lock.
            // Written as a return rather than a panic because production code in this crate does not
            // panic on a peer's input (AUDIT C223's bound is worth more than a tidier branch).
            return Err("the peer's registry entry vanished as it was admitted".to_string());
        };

        // A second session in the same direction is not a crossing, it is a duplicate: the older
        // one goes, so the peer's tables do not accumulate sessions to one object.
        let mut losers = Vec::new();
        if handle.dialed {
            if let Some((_, old)) = entry
                .dialed
                .replace((handle.own_pi.clone(), handle.clone()))
            {
                losers.push(old);
            }
        } else {
            // An accepted session read the peer's start-session during its handshake, so the identifier
            // the peer dialed with is always known. A caller that has none is registering the wrong
            // session, and the crossing could not be judged.
            let their_dialing = handle.peer_pi.clone().ok_or_else(|| {
                "an accepted session has no peer identifier, so the crossing cannot be judged"
                    .to_string()
            })?;
            if let Some((_, old)) = entry.accepted.replace((their_dialing, handle.clone())) {
                losers.push(old);
            }
        }

        let (Some((our_dialing, dialed_handle)), Some((their_dialing, accepted_handle))) =
            (&entry.dialed, &entry.accepted)
        else {
            return Ok(losers);
        };

        // The rule, on the two dialing sides: the lower one's connection dies.
        let our_dial_loses = crossed_hello(our_dialing, their_dialing) == CrossedHello::Abort;
        let this_loses = if our_dial_loses {
            handle.dialed
        } else {
            !handle.dialed
        };
        if this_loses {
            // Forget this session outright: its owner is about to drop it.
            if handle.dialed {
                entry.dialed = None;
            } else {
                entry.accepted = None;
            }
            return Err("crossed hellos".to_string());
        }
        // The *other* direction's session is the loser.
        let loser = if our_dial_loses {
            dialed_handle.clone()
        } else {
            accepted_handle.clone()
        };
        losers.push(loser);
        if our_dial_loses {
            entry.dialed = None;
        } else {
            entry.accepted = None;
        }
        Ok(losers)
    }

    /// How many peers the registry is tracking, live or not.
    ///
    /// Exposed because the map is the node's memory and the only thing that removes from it is
    /// [`SessionRegistry::forget`] — so "a peer is forgotten" is a claim about a number, and a claim
    /// about a number that nothing can read is a claim nothing checks (HAZOP row B3).
    pub fn tracked_peers(&self) -> usize {
        self.peers.lock().unpoison().len()
    }

    /// A live session to `peer`, if there is one — **the one the peer dialed, when there is a choice**.
    ///
    /// An enlivener has to reach an object at a peer it may already have a session with, and the
    /// conformance suite's handoff fixture is exactly that: it hands the enlivener a sturdyref to a
    /// peer whose session *it* dialed, and expects the fetch on that session. Preferring the accepted
    /// session is what makes that work; a dialed-only peer is reached on the session we dialed.
    pub fn live(&self, peer: &PeerLocator) -> Option<SessionHandle> {
        let peers = self.peers.lock().unpoison();
        let entry = peers.get(&peer_key(peer))?;
        entry
            .accepted
            .as_ref()
            .or(entry.dialed.as_ref())
            .map(|(_, handle)| handle.clone())
    }

    /// The session with a given shared id, if this peer has one.
    ///
    /// A third-party handoff names the session it is about by **id** (`handoff-give`'s `session`, and
    /// the receive's `receiving-session`), which is the only name both sides agree on — the ids are
    /// derived from both keys, and per-connection keys are not shared. So the registry is the only
    /// place that can turn an id back into a connection.
    pub fn by_id(&self, id: &Octets32) -> Option<SessionHandle> {
        let peers = self.peers.lock().unpoison();
        for (_, session) in peers.iter() {
            for (_, handle) in [session.dialed.as_ref(), session.accepted.as_ref()]
                .into_iter()
                .flatten()
            {
                if handle.id.as_ref() == Some(id) {
                    return Some(handle.clone());
                }
            }
        }
        None
    }

    /// Forget a session that has ended, so a later session to the same peer is not compared against
    /// a dead one.
    ///
    /// **And remove the peer when nothing is left.** Clearing the slot alone left the *key* — and the
    /// key is the peer's own `designator`/`transport` string, so a peer that varies its designator
    /// accumulated one dead entry per connection for the life of the process (measured: 3 000
    /// sessions, one entry each). The map is the node's memory, and this is the only thing that
    /// removes from it.
    ///
    /// **The match is on the handle's own identifier, not on the tuple's.** The two slots store
    /// different identifiers — the dialed one is keyed by *our* dialing identifier and the accepted
    /// one by the *peer's* — so comparing the caller's `own_pi` against the stored key cleared a
    /// dialed session and never an accepted one. The accepted slot is the common case (every peer
    /// that connects to us), so in practice **nothing was ever forgotten**: a session's handle stayed
    /// in the map after its loop ended. Found by
    /// `a_peer_whose_sessions_have_ended_is_forgotten`, which is why that test asserts the map's size
    /// and not only `live`.
    pub fn forget(&self, peer: &PeerLocator, own_pi: &Octets32, dialed: bool) {
        let mut peers = self.peers.lock().unpoison();
        let key = peer_key(peer);
        let Some(entry) = peers.get_mut(&key) else {
            return;
        };
        let slot = if dialed {
            &mut entry.dialed
        } else {
            &mut entry.accepted
        };
        if slot.as_ref().is_some_and(|(_, h)| h.own_pi == *own_pi) {
            *slot = None;
        }
        if entry.dialed.is_none() && entry.accepted.is_none() {
            peers.remove(&key);
        }
    }
}

/// The hint a locator carries the peer's Ed25519 verifying key in — the field [`peer_key`] reads in
/// preference to the designator (HAZOP rows C242/C243).
pub const VERIFY_HINT: &str = "verify";

/// The key a peer is registered under.
///
/// Designator and transport are the two fields `PeerLocator::same_peer` compares, and hints are
/// normally how to *reach* a peer rather than which peer it is — with one exception, and it is the
/// point of the exercise: **the `verify` hint** (HAZOP rows C242/C243).
///
/// **Why the hint is the exception.** A peer's designator is a self-assertion, signed by nothing but
/// the peer's own ephemeral session key, and it is public. The `verify` hint, by contrast, is a key
/// the transport *checks*: `noise` puts it in the SYN's cleartext prefix and refuses a session that
/// cannot prove it, and `websocket` checks the server's challenge response against it. So on a
/// *dialed* leg it is the key the handshake enforced, and on an *accepted* leg
/// [`accept_and_book`] writes the key the handshake **proved** into the same field. That is what
/// makes both legs of one peer agree on a name the peer cannot forge — the crossing rule compares the
/// two legs by this key, and would not if one were named by an assertion and the other by a proof.
///
/// A locator with no such hint — `tcp-testing-only`, `unix`, and every fixture — is keyed exactly as
/// it was before.
fn peer_key(peer: &PeerLocator) -> (String, String) {
    let designator = peer
        .hints
        .get(VERIFY_HINT)
        .cloned()
        .unwrap_or_else(|| peer.designator.clone());
    (designator, peer.transport.clone())
}

/// What an object that serves *one session* needs from it: the handle to send on, and the session
/// secret, which a third-party handoff's receive is signed with.
#[derive(Clone)]
pub struct SessionContext {
    pub handle: SessionHandle,
    pub secret: [u8; 32],
}

/// The slot a per-session object is handed so it can find **its own** session.
///
/// The bootstrap (and the fixtures published in it) is built *before* the session exists — `accept`
/// needs it — so it cannot be given the handle at construction. It is filled in immediately after the
/// session is split, and anything that reads it (a deposit, a handoff give) arrives on that session,
/// so it is always set by the time it is read. `None` means exactly that: the session does not exist
/// yet, and a delivery could not have arrived.
pub type SessionSlot = Arc<Mutex<Option<SessionContext>>>;

/// **Where the peer on this session is**, when the transport knows (Law 62, AUDIT C225).
///
/// A dial a peer asks for is judged against this: a remote peer may not make this node reach the
/// node's own loopback. `None` when the slot is empty or the transport cannot say — the policy treats
/// both as "cannot be judged" and dials as it did before.
pub fn session_origin(session: &SessionSlot) -> Option<std::net::SocketAddr> {
    session
        .lock()
        .unpoison()
        .as_ref()
        .and_then(|context| context.handle.peer_address)
}

/// A slot, empty until its session exists.
pub fn session_slot() -> SessionSlot {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locator(designator: &str, verify: Option<&str>) -> PeerLocator {
        let mut hints = std::collections::BTreeMap::new();
        if let Some(key) = verify {
            hints.insert(VERIFY_HINT.to_string(), key.to_string());
        }
        PeerLocator {
            designator: designator.to_string(),
            transport: "noise".to_string(),
            hints,
        }
    }

    /// **A peer is keyed by the key its transport proved, not by the name it asserted** (HAZOP rows
    /// C242/C243).
    ///
    /// This is what makes the two legs of one peer agree — the leg we dial is keyed by the `verify`
    /// hint the handshake enforced, and the leg we accept by the key `accept_and_book` wrote there
    /// after proving it — so the crossing rule compares like with like. Without it, a peer that
    /// asserted another peer's public name would be filed under that name and could evict it.
    #[test]
    fn a_peer_is_keyed_by_the_key_its_transport_proved() {
        // The same peer reached two ways: the second locator spells the designator differently,
        // because hints (and designators) are how a peer is *reached* rather than which peer it is.
        let named = locator("base16-key", Some("base16-key"));
        let reached = locator("some-other-name", Some("base16-key"));
        assert_eq!(peer_key(&named), peer_key(&reached));

        // And a peer that writes the honest one's name into its designator, with a key of its own, is
        // a different peer — it is filed under the key it proved, so it cannot collide with the
        // session it named.
        let liar = locator("base16-key", Some("a-different-key"));
        assert_ne!(peer_key(&named), peer_key(&liar));

        // A locator that carries no key at all is keyed exactly as it was before this change:
        // `tcp-testing-only`, `unix`, and every fixture. (A peer cannot use the fallback to evade the
        // rule on a transport that *does* check a key: `noise` and `websocket` both refuse to dial a
        // locator with no `verify` hint, so such a leg never reaches the registry.)
        assert_eq!(peer_key(&locator("rnode-ocapn", None)).0, "rnode-ocapn");
    }
}
