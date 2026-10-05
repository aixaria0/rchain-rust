//! Owning a session — the two things the conformance suite's stage-6 fixtures exercise, in Rust, so
//! they fail in `cargo test` rather than only in a Python run.
//!
//! **The obstacle this file exists for.** A session's socket and its export/answer tables are
//! single-owner: `Session::run` owns them, and an object answers by returning an `Act` the loop
//! performs. That is enough for a peer that only answers. It is not enough to *dial out and wait*,
//! which is what a sturdyref enlivener does inside a delivery on another session — one task owns
//! each socket, and the two talk through a handle (`owner::SessionHandle`) instead of a lock.
//!
//! Four gates, because they fail independently:
//!
//! 1. a session we dialed can send a delivery **whose answer we await**, and the answer arrives
//!    through that session's own loop (`op_deliver`'s pipelining, plus the answer we registered as
//!    the delivery's `resolve-me-desc`);
//! 2. when the same two peers have dialed each other, **exactly one** of the two sessions is
//!    aborted, and which one is decided by the identifier rule — asserted from the peer's own socket,
//!    because the loser's `op:abort` is a message on the wire;
//! 3. **a peer that never speaks is refused, not held** (HAZOP row B1) — the handshake bound, which
//!    is what stops one connection pinning a task for ever;
//! 4. **a peer whose sessions have all ended is forgotten** (row B3) — `forget` must remove the key,
//!    because the key is the peer's own `designator`, and a map that only grows is the node's memory.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use rchain_ocapn::bootstrap::Bootstrap;
use rchain_ocapn::captp::{Deliver, Desc};
use rchain_ocapn::conn::{Act, ConnectionError, Export, Identity, Session};
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

/// **A peer that connects and says nothing is refused, and refused promptly** (HAZOP row B1).
///
/// This is the slow-loris the red team measured: 3 000 idle connections, +41 MB, one pinned task
/// each, the RSS never returned. The bound is what makes that a closed socket instead — and it is
/// tested through [`Session::accept_deferred_within`] rather than by waiting the production 30 s,
/// because a security bound nobody can afford to test is a bound that gets removed.
///
/// The refused peer must also be **told**: the accept path writes `op:abort`, so the silent
/// connector learns why rather than seeing a bare close.
#[tokio::test]
async fn a_peer_that_never_speaks_is_refused_within_the_handshake_bound() {
    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let identity = Identity::from_seed([31u8; 32], locator(port)).unwrap();

    // The client connects and sends nothing at all — the attack.
    let mut silent = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");

    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        let conn = listener
            .accept_incoming_connection()
            .await
            .expect("accept the silent peer");
        Session::accept_deferred_within(
            conn,
            &identity,
            Arc::new(Bootstrap::default()),
            Duration::from_millis(200),
        )
        .await
    })
    .await
    .expect("the bound fires well inside the test's own wait");

    // `Session` is not `Debug` (it owns a boxed connection), so the refusal is matched, not
    // `expect_err`'d.
    let refused = match outcome {
        Ok(_) => panic!("a peer that never speaks must be refused, not held"),
        Err(e) => e,
    };
    assert!(
        matches!(refused, ConnectionError::Handshake(_)),
        "the refusal is a handshake refusal: {refused:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "and it fires on the bound, not on the test's timeout: {:?}",
        started.elapsed()
    );

    // The peer is told. Read what it received: an `op:abort`, not a bare close.
    use tokio::io::AsyncReadExt;
    let mut got = vec![0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(2), silent.read(&mut got))
        .await
        .expect("the peer is told why")
        .expect("read");
    let text = String::from_utf8_lossy(&got[..n]).to_string();
    assert!(
        text.contains("op:abort"),
        "the silent peer should be refused in words: {text:?}"
    );
}

/// **A peer whose sessions have all ended is forgotten** (HAZOP row B3): `forget` must remove the
/// key, not just clear the slot, because the key is the peer's own `designator` and a peer that
/// varies it accumulates one dead entry per connection for the process's life.
#[tokio::test]
async fn a_peer_whose_sessions_have_ended_is_forgotten() {
    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let registry = Arc::new(SessionRegistry::default());

    // A peer dials us: this is the leg the registry books.
    let dialer = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let conn = dialer
        .new_outgoing_connection(&locator(port))
        .await
        .unwrap();
    let peer_identity = Identity::from_seed([41u8; 32], locator(0)).unwrap();
    let _peer_side = Session::dial_deferred(conn, &peer_identity, Arc::new(Bootstrap::default()))
        .await
        .expect("we send our start-session and do not wait");

    let accepted_conn = listener.accept_incoming_connection().await.unwrap();
    let our_identity = Identity::from_seed([42u8; 32], locator(port)).unwrap();
    let (handle, _loop_, _context, peer) = accept_and_book(
        accepted_conn,
        &our_identity,
        Arc::new(Bootstrap::default()),
        &registry,
    )
    .await
    .expect("the accepted leg is booked");
    assert_eq!(registry.tracked_peers(), 1, "the peer is booked");

    // The session ends: forgetting it must take the entry with it.
    registry.forget(&peer, &handle.own_pi, handle.dialed);
    assert!(
        registry.live(&peer).is_none(),
        "a forgotten session is not live"
    );
    assert_eq!(
        registry.tracked_peers(),
        0,
        "and the peer's own map entry goes too — this is the whole of row B3"
    );
}

/// **A peer cannot grow a session's export table past its cap** (HAZOP row B2, AUDIT C223).
///
/// The red team measured three tables a peer grows without limit — the answer table by naming a fresh
/// `answer_pos`, the gift store by a fresh gift id, and this one by asking for objects. This is the
/// export table's, because it is the one the *protocol* makes a peer drive: every `fetch` and every
/// returned capability exports something.
///
/// The assertions that matter are the last two: the delivery is refused **with a reason naming the
/// bound** (not dropped, which would be the C18 class), and **the session is still alive** — a table
/// at its cap is a bounded table, not a broken one.
#[tokio::test]
async fn the_export_table_refuses_the_delivery_that_would_grow_it_past_its_cap() {
    // A far end that hands back an object for every `"ask …"` — so the peer drives `insert_export`
    // with a message it chose — and a plain value otherwise, which is how the liveness probe at the
    // end is told apart from the growth attempts.
    struct HandsBackAnObject;

    #[async_trait]
    impl Export for HandsBackAnObject {
        async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
            match args.first() {
                Some(Value::String(s)) if s.starts_with("ask ") => Ok(Act::object(Arc::new(Pong))),
                _ => Ok(Act::value(Value::String("pong".to_string()))),
            }
        }
    }

    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let far_end = tokio::spawn(async move {
        let conn = listener.accept_incoming_connection().await.unwrap();
        let identity = Identity::from_seed([51u8; 32], locator(port)).unwrap();
        let mut session = Session::accept(conn, &identity, Arc::new(HandsBackAnObject))
            .await
            .unwrap();
        let _ = session.run().await;
    });

    let dialer = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let conn = dialer
        .new_outgoing_connection(&locator(port))
        .await
        .unwrap();
    let identity = Identity::from_seed([52u8; 32], locator(0)).unwrap();
    let mut client = Session::dial(conn, &identity, Arc::new(Bootstrap::default()))
        .await
        .expect("handshake");

    // Each delivery asks for its reply at a fresh import position of ours, so the *far end* exports
    // one object per message: the table under test is the one this test is the peer of.
    let cap = rchain_ocapn::capacity::MAX_EXPORTS;
    let mut refused: Option<Value> = None;
    for i in 0..(cap + 2) {
        let ask = Deliver {
            to: rchain_ocapn::captp::Desc::Export(0u64.into()),
            args: vec![Value::String(format!("ask {i}"))],
            answer_pos: None,
            resolve_me_desc: Some(Desc::ImportObject((i as u64).into())),
        };
        client.send_message(&ask.to_syrup()).await.expect("send");
        let reply = client
            .recv_message()
            .await
            .expect("read")
            .expect("a reply, not a closed connection");
        let delivered = Deliver::from_syrup(&reply).expect("a delivery");
        if let Value::Symbol(verb) = &delivered.args[0] {
            if verb == "break" {
                refused = Some(delivered.args[1].clone());
                break;
            }
        }
    }

    let reason = refused.expect("the table's cap must refuse a delivery, not absorb it");
    let Value::String(reason) = reason else {
        panic!("a break carries its reason");
    };
    assert!(
        reason.contains("export table is full"),
        "the refusal names the bound: {reason}"
    );

    // Bounded, not broken: the session still answers.
    let after = Deliver {
        to: rchain_ocapn::captp::Desc::Export(0u64.into()),
        args: vec![Value::String("still there?".to_string())],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(9999u64.into())),
    };
    client.send_message(&after.to_syrup()).await.expect("send");
    let reply = client
        .recv_message()
        .await
        .expect("read")
        .expect("the session is alive after a refused delivery");
    let delivered = Deliver::from_syrup(&reply).expect("a delivery");
    assert_eq!(
        delivered.args[0],
        Value::Symbol("fulfill".into()),
        "the session keeps serving at its cap: {delivered:?}"
    );

    drop(client);
    far_end.abort();
}

/// **A released export is released** (HAZOP row B2, AUDIT C223 residue 1).
///
/// `op:gc-exports` used to be dropped by label — the struct had no `from_syrup` at all — so a peer's
/// explicit release was a no-op and only the table caps bounded the table. Now the release removes the
/// position, and the proof is that the position stops resolving.
#[tokio::test]
async fn a_released_export_no_longer_resolves() {
    // A far end that hands back an object for `"ask …"`: each reply populates *its* export table.
    struct HandsBackAnObject;

    #[async_trait]
    impl Export for HandsBackAnObject {
        async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
            match args.first() {
                Some(Value::String(_)) => Ok(Act::object(Arc::new(Pong))),
                _ => Ok(Act::value(Value::String("pong".to_string()))),
            }
        }
    }

    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let far_end = tokio::spawn(async move {
        let conn = listener.accept_incoming_connection().await.unwrap();
        let identity = Identity::from_seed([61u8; 32], locator(port)).unwrap();
        let mut session = Session::accept(conn, &identity, Arc::new(HandsBackAnObject))
            .await
            .unwrap();
        let _ = session.run().await;
    });

    let dialer = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let conn = dialer
        .new_outgoing_connection(&locator(port))
        .await
        .unwrap();
    let identity = Identity::from_seed([62u8; 32], locator(0)).unwrap();
    let mut client = Session::dial(conn, &identity, Arc::new(Bootstrap::default()))
        .await
        .expect("handshake");

    // One delivery, one object handed back: the far end's export 1 (0 is its bootstrap).
    let ask = Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![Value::String("ask".to_string())],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
    };
    client.send_message(&ask.to_syrup()).await.expect("send");
    let reply = client.recv_message().await.unwrap().unwrap();
    let delivered = Deliver::from_syrup(&reply).unwrap();
    let Value::Record(desc) = &delivered.args[1] else {
        panic!("expected a descriptor, got {:?}", delivered.args[1]);
    };
    let Value::Int(position) = &desc[1] else {
        panic!("expected a position in {desc:?}");
    };
    let position = position.magnitude().clone();

    // The peer releases it. This is the message that used to be dropped.
    let gc = rchain_ocapn::captp::OpGcExports {
        positions: vec![position.clone()],
        wire_deltas: vec![1u64.into()],
    };
    client
        .send_message(&gc.to_syrup())
        .await
        .expect("send the release");

    // And the released position no longer resolves: the far end aborts, naming the reason.
    let after = Deliver {
        to: Desc::Export(position),
        args: vec![Value::String("still there?".to_string())],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(1u64.into())),
    };
    client.send_message(&after.to_syrup()).await.expect("send");
    let reply = client.recv_message().await.unwrap().unwrap();
    let Value::Record(fields) = &reply else {
        panic!("expected a record, got {reply:?}");
    };
    assert!(
        matches!(fields.first(), Some(Value::Symbol(label)) if label == "op:abort"),
        "a released export must not resolve: {reply:?}"
    );
    assert!(
        matches!(fields.get(1), Some(Value::String(reason)) if reason.contains("no such export")),
        "and the abort must say why: {reply:?}"
    );

    drop(client);
    far_end.abort();
}

/// **A re-used answer position is refused, not re-pointed** (HAZOP row D5, AUDIT C223 residue 3).
///
/// An answer position is the sender's own promise slot, and it hands `desc:answer N` to third parties;
/// silently pointing N at a different delivery breaks a reference the peer may still hold. Before the
/// fix the second delivery replaced the mapping and nothing said so.
#[tokio::test]
async fn a_reused_answer_position_is_refused() {
    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let far_end = tokio::spawn(async move {
        let conn = listener.accept_incoming_connection().await.unwrap();
        let identity = Identity::from_seed([71u8; 32], locator(port)).unwrap();
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
    let identity = Identity::from_seed([72u8; 32], locator(0)).unwrap();
    let mut client = Session::dial(conn, &identity, Arc::new(Bootstrap::default()))
        .await
        .expect("handshake");

    let with_answer = |text: &str| Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![Value::String(text.to_string())],
        answer_pos: Some(0u64.into()),
        resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
    };

    // The first delivery claims answer position 0, and is served.
    client
        .send_message(&with_answer("first").to_syrup())
        .await
        .expect("send");
    let reply = client.recv_message().await.unwrap().unwrap();
    let delivered = Deliver::from_syrup(&reply).unwrap();
    assert_eq!(
        delivered.args[0],
        Value::Symbol("fulfill".into()),
        "the first delivery on a fresh position is served: {delivered:?}"
    );

    // The second re-uses it. The session must say so and stop.
    client
        .send_message(&with_answer("second").to_syrup())
        .await
        .expect("send");
    let reply = client.recv_message().await.unwrap().unwrap();
    let Value::Record(fields) = &reply else {
        panic!("expected a record, got {reply:?}");
    };
    assert!(
        matches!(fields.first(), Some(Value::Symbol(label)) if label == "op:abort"),
        "re-using an answer position must abort the session: {reply:?}"
    );
    assert!(
        matches!(fields.get(1), Some(Value::String(reason)) if reason.contains("already used")),
        "and the abort must name the position: {reply:?}"
    );

    drop(client);
    far_end.abort();
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
