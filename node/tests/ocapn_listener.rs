//! The node's OCapN listener (issue #249): a foreign peer dials a running node.
//!
//! This is the listener's own gate. The bridge that turns a delivery into a signed deploy is the
//! next unit, so what this proves is deliberately smaller and checkable on its own: a node that is
//! configured with `api-server.ocapn-listen` answers a CapTP handshake and serves its fixtures.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use rchain_ocapn::bootstrap::Bootstrap;
use rchain_ocapn::captp::{Deliver, Desc};
use rchain_ocapn::conn::{Identity, Session};
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::syrup::Value;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

/// The echo fixture's swiss number, as the conformance suite spells it.
const ECHO_SWISS: &[u8] = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w";

/// Wait until the node's OCapN port accepts a connection. `start` returns as soon as the program is
/// spawned, and the listeners bind after the store replay — the same gap `wait_for_genesis` covers
/// for the HTTP API. The probe connection is dropped at once; the listener treats the close as a
/// session that ended, which it is.
async fn wait_for_ocapn(port: u16) {
    for _ in 0..300 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the OCapN listener on 127.0.0.1:{port} never came up");
}

fn locator(port: u16) -> PeerLocator {
    PeerLocator {
        designator: "rnode".to_string(),
        transport: "tcp-testing-only".to_string(),
        hints: BTreeMap::from([
            ("host".to_string(), "127.0.0.1".to_string()),
            ("port".to_string(), port.to_string()),
        ]),
    }
}

#[test]
fn a_peer_dials_the_node_and_fetches_a_fixture() {
    let dir = common::temp_dir("ocapn-listener");
    // [http, admin-http, grpc-internal, protocol, grpc-external, ocapn]
    let ports = common::free_ports(6);
    let mut conf = common::deploy_conf(&dir, &ports);
    conf.api_server.ocapn_listen = Some(format!("127.0.0.1:{}", ports[5]));

    common::test_runtime().block_on(async {
        let node = common::start(&conf, ports[2] as u16, ports[0] as u16).await;
        wait_for_ocapn(ports[5] as u16).await;

        // Dial the node the way any OCapN peer would: a fresh session key, our own empty bootstrap.
        let dialer = TcpTestingOnly::bind("127.0.0.1:0")
            .await
            .expect("bind the dialing side");
        let connection = dialer
            .new_outgoing_connection(&locator(ports[5] as u16))
            .await
            .expect("dial the node");
        let identity = Identity::fresh(locator(0)).expect("a session key");
        let mut client = Session::dial(connection, &identity, Arc::new(Bootstrap::default()))
            .await
            .expect("the node should complete the handshake");

        // Fetch the echo fixture from the node's bootstrap (its export 0), asking for the reply at
        // our own export 0 — the shape the conformance suite's `fetch_object` uses.
        let fetch = Deliver {
            to: Desc::Export(0u64.into()),
            args: vec![
                Value::Symbol("fetch".into()),
                Value::Bytes(ECHO_SWISS.to_vec()),
            ],
            answer_pos: None,
            resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
        };
        client
            .send_message(&fetch.to_syrup())
            .await
            .expect("send the fetch");

        let reply = client
            .recv_message()
            .await
            .expect("read the reply")
            .expect("a reply, not a closed connection");
        let delivered = Deliver::from_syrup(&reply).expect("a delivery");
        assert_eq!(delivered.to, Desc::Export(0u64.into()));
        assert_eq!(
            delivered.args[0],
            Value::Symbol("fulfill".into()),
            "the fixture should have been fulfilled, not refused"
        );
        assert!(
            matches!(delivered.args[1], Value::Record(_)),
            "the node should hand back a descriptor for the object; got {:?}",
            delivered.args[1]
        );

        drop(client);
        node.shutdown();
    });
    let _ = std::fs::remove_dir_all(&dir);
}
