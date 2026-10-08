//! Two of our own sessions complete a CapTP handshake and one fetches an object from the other's
//! bootstrap — stages 0 and 1 end to end, in Rust, over the real netlayer.
//!
//! The client drives its side by hand (`send_message` / `recv_message`) so it can assert on the
//! exact messages, the way the conformance suite does; the server runs its loop and answers.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use rchain_ocapn::bootstrap::Bootstrap;
use rchain_ocapn::captp::{Deliver, Desc};
use rchain_ocapn::conn::{Act, Export, Identity, Session};
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::syrup::Value;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

/// The echo object the conformance suite fetches at this swiss number.
const ECHO_SWISS: &[u8] = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w";

/// Replies with its arguments, the shape the suite's echo test expects.
struct Echo;

#[async_trait]
impl Export for Echo {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        Ok(Act::value(Value::List(args.to_vec())))
    }
}

fn locator(port: u16) -> PeerLocator {
    PeerLocator {
        designator: "peer".into(),
        transport: "tcp-testing-only".into(),
        hints: BTreeMap::from([
            ("host".to_string(), "127.0.0.1".to_string()),
            ("port".to_string(), port.to_string()),
        ]),
    }
}

fn bootstrap_with_echo() -> Arc<dyn Export> {
    let mut directory: BTreeMap<Vec<u8>, Arc<dyn Export>> = BTreeMap::new();
    directory.insert(ECHO_SWISS.to_vec(), Arc::new(Echo));
    Arc::new(Bootstrap::new(directory))
}

#[tokio::test]
async fn two_sessions_handshake_and_the_client_fetches_from_the_bootstrap() {
    let listener = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (id_tx, id_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let conn = listener.accept_incoming_connection().await.unwrap();
        let identity = Identity::from_seed([1u8; 32], locator(port)).unwrap();
        let mut session = Session::accept(conn, &identity, bootstrap_with_echo())
            .await
            .unwrap();
        let _ = id_tx.send(session.id);
        // Runs until the client goes away.
        let _ = session.run().await;
    });

    let dialer = TcpTestingOnly::bind("127.0.0.1:0").await.unwrap();
    let conn = dialer
        .new_outgoing_connection(&locator(port))
        .await
        .unwrap();
    let identity = Identity::from_seed([2u8; 32], locator(0)).unwrap();
    let mut client = Session::dial(conn, &identity, Arc::new(Bootstrap::default()))
        .await
        .unwrap();

    // Both sides must derive the same session id — from each other's *Syrup-encoded* key.
    assert_eq!(client.id, id_rx.await.unwrap());

    // `fetch` the echo object from the server's bootstrap (its export 0), asking for the reply at
    // our own export 0 — exactly the suite's `fetch_object` shape.
    let fetch = Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![
            Value::Symbol("fetch".into()),
            Value::Bytes(ECHO_SWISS.to_vec()),
        ],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
    };
    client.send_message(&fetch.to_syrup()).await.unwrap();

    // The reply is a fulfilment carrying a descriptor for the object, which the server exported at
    // position 1 (its bootstrap holds 0).
    let reply = client.recv_message().await.unwrap().unwrap();
    let delivered = Deliver::from_syrup(&reply).unwrap();
    assert_eq!(delivered.to, Desc::Export(0u64.into()));
    assert_eq!(delivered.args[0], Value::Symbol("fulfill".into()));
    assert_eq!(
        delivered.args[1],
        Desc::ImportObject(1u64.into()).to_syrup()
    );

    // A fetch of an unknown swiss number must break, not hang.
    let missing = Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![
            Value::Symbol("fetch".into()),
            Value::Bytes(b"no-such-object".to_vec()),
        ],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(1u64.into())),
    };
    client.send_message(&missing.to_syrup()).await.unwrap();
    let reply = client.recv_message().await.unwrap().unwrap();
    let delivered = Deliver::from_syrup(&reply).unwrap();
    assert_eq!(delivered.args[0], Value::Symbol("break".into()));

    drop(client); // let the server's loop end
    server.await.unwrap();
}
