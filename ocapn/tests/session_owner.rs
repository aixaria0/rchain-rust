//! Owning a session — the two things the conformance suite's stage-6 fixtures exercise, in Rust, so
//! they fail in `cargo test` rather than only in a Python run.
//!
//! **The obstacle this file exists for.** A session's socket and its export/answer tables are
//! single-owner: `Session::run` owns them, and an object answers by returning an `Act` the loop
//! performs. That is enough for a peer that only answers. It is not enough to *dial out and wait*,
//! which is what a sturdyref enlivener does inside a delivery on another session — one task owns
//! each socket, and the two talk through a handle (`owner::SessionHandle`) instead of a lock.
//!
//! Two gates, because they fail independently:
//!
//! 1. a session we dialed can send a delivery **whose answer we await**, and the answer arrives
//!    through that session's own loop (`op_deliver`'s pipelining, plus the answer we registered as
//!    the delivery's `resolve-me-desc`);
//! 2. when the same two peers have dialed each other, **exactly one** of the two sessions is
//!    aborted, and which one is decided by the identifier rule — asserted from the peer's own socket,
//!    because the loser's `op:abort` is a message on the wire.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use rchain_ocapn::bootstrap::Bootstrap;
use rchain_ocapn::conn::{Act, Export, Identity, Session};
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::owner::{accept_and_book, HandOff, SessionRegistry};
use rchain_ocapn::proxy::catcher;
use rchain_ocapn::session_id::{crossed_hello, CrossedHello};
use rchain_ocapn::syrup::Value;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

/// Answers every delivery with the same string: enough to be the far end of a dial-and-wait.
struct Pong;

#[async_trait]
impl Export for Pong {
    async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
        Ok(Act::value(Value::String("pong".to_string())))
    }
}

fn locator(port: u16) -> PeerLocator {
    PeerLocator {
        designator: "peer".to_string(),
        transport: "tcp-testing-only".to_string(),
        hints: BTreeMap::from([
            ("host".to_string(), "127.0.0.1".to_string()),
            ("port".to_string(), port.to_string()),
        ]),
    }
}

/// **We dial, and the answer arrives through the loop that owns the socket.**
///
/// The delivery carries a `resolve-me` export — the catcher — and the peer fulfils it with a
/// delivery of its own. That is the whole hand-off/across-sessions mechanism in miniature: a task
/// that does not own a socket asks for a delivery on it, and then *awaits* a message that the socket
/// owner will receive.
#[tokio::test]
async fn a_dialed_session_awaits_an_answer_through_its_own_loop() {
    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // The far end, driven by the library's own loop: it pongs whatever it is delivered.
    let peer = tokio::spawn(async move {
        let conn = listener.accept_incoming_connection().await.unwrap();
        let identity = Identity::from_seed([11u8; 32], locator(port)).unwrap();
        let mut session = Session::accept(conn, &identity, Arc::new(Pong))
            .await
            .unwrap();
        let _ = session.run().await;
    });

    let dialer = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let conn = dialer
        .new_outgoing_connection(&locator(port))
        .await
        .unwrap();
    let identity = Identity::from_seed([12u8; 32], locator(0)).unwrap();
    let session = Session::dial(conn, &identity, Arc::new(Bootstrap::default()))
        .await
        .expect("the peer completes the handshake");
    let (handle, loop_, _context) = session.split();
    tokio::spawn(async move {
        let _ = loop_.run().await;
    });

    let (catcher, rx) = catcher();
    handle
        .deliver(HandOff {
            to: rchain_ocapn::captp::Desc::Export(0u64.into()),
            args: vec![Value::String("ping".to_string())],
            resolve_me: Some(catcher),
        })
        .await
        .expect("the session is alive");

    let answer = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("the answer arrives while we await it")
        .expect("the catcher is not dropped");
    assert_eq!(
        answer,
        Ok(Value::String("pong".to_string())),
        "the peer's fulfilment should reach the catcher on the session we dialed"
    );

    drop(handle);
    peer.abort();
}

/// **A crossing aborts the leg the rule names, and the peer sees it.**
///
/// The peer dials us *and* we dial the peer — the two connections a handoff's dial-back produces.
/// Both peers must agree which one dies, and the rule is the *dialing* identifiers: the lower of the
/// two dialing sides aborts its connection. Asserted from the peer's own socket, because the loser's
/// `op:abort` is a message, and a session dropped without writing it would look alive.
#[tokio::test]
async fn a_crossing_aborts_the_leg_the_rule_names() {
    let ours = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let our_port = ours.local_addr().unwrap().port();
    let theirs = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let their_port = theirs.local_addr().unwrap().port();

    // The peer dials us first: this is the leg we will *accept*.
    let their_out = theirs
        .new_outgoing_connection(&locator(our_port))
        .await
        .unwrap();
    let peer_identity = Identity::from_seed([21u8; 32], locator(their_port)).unwrap();
    let mut peer_side =
        Session::dial_deferred(their_out, &peer_identity, Arc::new(Bootstrap::default()))
            .await
            .expect("we send our start-session and do not wait");

    let registry = Arc::new(SessionRegistry::default());
    let accepted_conn = ours.accept_incoming_connection().await.unwrap();
    let our_identity = Identity::from_seed([22u8; 32], locator(our_port)).unwrap();
    let (accepted, accepted_loop, _context, peer) = accept_and_book(
        accepted_conn,
        &our_identity,
        Arc::new(Bootstrap::default()),
        &registry,
    )
    .await
    .expect("the accepted leg is booked");
    assert_eq!(
        peer,
        locator(their_port),
        "the peer's own advertised location"
    );

    // Now we dial the same peer: the crossing.
    let our_out = ours.new_outgoing_connection(&peer).await.unwrap();
    let dialing_identity = Identity::from_seed([23u8; 32], locator(our_port)).unwrap();
    let dialed = Session::dial_deferred(our_out, &dialing_identity, Arc::new(Bootstrap::default()))
        .await
        .expect("we dial the peer");
    let (dialed_handle, dialed_loop, _) = dialed.split();

    // The peer is doing the same thing from its side, so it must send its start-session on the leg it
    // dialed; read it, so the dialed session's identifiers are known to both sides.
    tokio::time::timeout(Duration::from_secs(10), peer_side.complete_handshake())
        .await
        .expect("the peer answers on the leg it dialed")
        .expect("our start-session is a start-session");

    // The rule: the session dialed by the lower of the two *dialing* identifiers is the loser.
    let our_dialing = dialed_handle.own_pi.clone();
    let their_dialing = accepted
        .peer_pi
        .clone()
        .expect("an accepted session has the peer's identifier");
    let our_dialed_leg_loses = crossed_hello(&our_dialing, &their_dialing) == CrossedHello::Abort;

    match registry.admit(&peer, &dialed_handle) {
        Ok(losers) => {
            // Our dialed leg won; the accepted one is named as the loser and must be stopped — by
            // *us*, because its owner is the task that booked it and this is not that task.
            assert!(
                !our_dialed_leg_loses,
                "admit kept the dialed leg while the rule says it loses"
            );
            for loser in losers {
                loser.abort().await;
            }
            let accepted_task = tokio::spawn(async move {
                let _ = accepted_loop.run().await;
            });
            let _ = tokio::time::timeout(Duration::from_secs(10), accepted_task).await;
            tokio::spawn(async move {
                let _ = dialed_loop.run().await;
            });
        }
        Err(reason) => {
            assert!(
                our_dialed_leg_loses,
                "admit dropped the dialed leg while the rule says it wins ({reason})"
            );
            // The loser writes its own `op:abort`, which is why its loop runs once.
            dialed_handle.abort().await;
            let _ = tokio::time::timeout(Duration::from_secs(10), dialed_loop.run()).await;
        }
    }

    // Whichever leg died, the **peer** sees the `op:abort` on the matching socket: the leg it dialed
    // if we aborted our accepted one, the leg it accepted if we aborted our dialed one.
    let died_on_the_peers_dialed_leg =
        is_abort_within(&mut peer_side, Duration::from_secs(5)).await;
    assert_eq!(
        died_on_the_peers_dialed_leg, !our_dialed_leg_loses,
        "the abort must be written on the leg the rule names, and on the peer's socket"
    );

    drop(accepted);
    drop(dialed_handle);
}

/// Whether the next message the peer reads is an `op:abort` — read with a bound, so a leg that is
/// simply still open (the survivor) fails the assertion rather than hanging the test.
async fn is_abort_within(session: &mut Session, within: Duration) -> bool {
    let Ok(Ok(Some(message))) = tokio::time::timeout(within, session.recv_message()).await else {
        return false;
    };
    matches!(
        message,
        Value::Record(ref fields)
            if matches!(fields.first(), Some(Value::Symbol(label)) if label == "op:abort")
    )
}
