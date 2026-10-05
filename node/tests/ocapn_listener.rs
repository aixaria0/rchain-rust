//! The node's OCapN listener and its bridge (issue #249): a foreign peer dials a running node.
//!
//! Two gates, because they fail independently. The first is the listener's own: a node configured
//! with `api-server.ocapn-listen` answers a CapTP handshake and serves its fixtures. The second is
//! the bridge: a delivery to a chain-backed capability becomes a signed deploy, and that deploy
//! lands in a block, and the value the deployed contract wrote to the deploy's reply channel comes
//! back as the CapTP promise's fulfilment — the balance it reads is the wallet's, less the phlo that
//! very deploy spent.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use rchain_casper::protocol::client::{GrpcProposeService, ProposeService};
use rchain_node::api::ocapn::REV_VAULT_BALANCE_SWISS;
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

/// Wait until the node is out of read-only mode, i.e. genesis has been created and announced. The
/// OCapN port binds at "replay complete — starting listeners", which is *before* that, so a propose
/// driven straight after `wait_for_ocapn` is answered `ReadOnlyMode`. `deploy_block.rs` waits the
/// same way, over the HTTP API.
async fn wait_for_genesis(base: &str) {
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Ok(resp) = client.get(format!("{base}/api/blocks")).send().await {
            if resp.status().is_success() {
                if let Ok(serde_json::Value::Array(blocks)) = resp.json::<serde_json::Value>().await
                {
                    if !blocks.is_empty() {
                        return;
                    }
                }
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "genesis never appeared: {base}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
        let node = common::start(&conf, ports[2], ports[0]).await;
        wait_for_ocapn(ports[5]).await;

        // Dial the node the way any OCapN peer would: a fresh session key, our own empty bootstrap.
        let dialer = TcpTestingOnly::bind("127.0.0.1:0")
            .await
            .expect("bind the dialing side");
        let connection = dialer
            .new_outgoing_connection(&locator(ports[5]))
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

/// The bridge: a CapTP delivery to a chain-backed capability becomes a signed deploy, and the value
/// that deploy wrote to its reply channel comes back as the CapTP promise's fulfilment.
#[test]
fn a_captp_delivery_to_a_chain_backed_capability_resolves_from_a_block() {
    let dir = common::temp_dir("ocapn-bridge");
    let ports = common::free_ports(6);
    let mut conf = common::deploy_conf(&dir, &ports);
    conf.api_server.ocapn_listen = Some(format!("127.0.0.1:{}", ports[5]));
    // The bridge signs with the node's deployer key and needs a block to carry the deploy.
    conf.dev_mode = true;
    conf.dev.deployer_private_key = Some(common::VALIDATOR_PRIV_HEX.to_string());
    conf.propose_on_deploy = true;

    common::test_runtime().block_on(async {
        let node = common::start(&conf, ports[2], ports[0]).await;
        wait_for_ocapn(ports[5]).await;
        wait_for_genesis(&format!("http://127.0.0.1:{}", ports[0])).await;

        let dialer = TcpTestingOnly::bind("127.0.0.1:0")
            .await
            .expect("bind the dialing side");
        let connection = dialer
            .new_outgoing_connection(&locator(ports[5]))
            .await
            .expect("dial the node");
        let identity = Identity::fresh(locator(0)).expect("a session key");
        let mut client = Session::dial(connection, &identity, Arc::new(Bootstrap::default()))
            .await
            .expect("the node should complete the handshake");

        // 1. Fetch the chain-backed capability from the bootstrap.
        let capability = fetch(&mut client, REV_VAULT_BALANCE_SWISS).await;

        // 2. Deliver to it: one REV address in. Everything after this is the node's own doing — it
        //    builds the term, signs the deploy, submits it, and waits for the block.
        let call = Deliver {
            to: capability,
            args: vec![Value::String(common::DEPLOYER_REV_ADDR.to_string())],
            answer_pos: None,
            resolve_me_desc: Some(Desc::ImportObject(1u64.into())),
        };
        client
            .send_message(&call.to_syrup())
            .await
            .expect("send the call");

        // The bridged deploy lands because **the node proposes it**: `propose_on_deploy` is on, and
        // that is the mechanism this test is exercising. So this explicit nudge — the shape
        // `deploy_block.rs` uses, kept as a second chance — normally arrives while the node's own
        // propose is already running, and the node answers `Failure: another propose is in progress`.
        // **That is not a failure here**, and asserting on it is how this test failed twice on CI: the
        // refusal says a propose is in flight, which is a block on its way, and the assertion that
        // matters is the reply below — the only thing that says a block carried the *deploy*. Both
        // answers have been measured here; only these two are acceptable, and neither is required.
        let propose = GrpcProposeService::connect("127.0.0.1", ports[2] as i32, 16 * 1024 * 1024)
            .await
            .expect("propose gRPC");
        match propose.propose(false).await {
            Ok(_) => {}
            Err(errors)
                if errors
                    .iter()
                    .any(|e| e.contains("another propose is in progress")) => {}
            Err(errors) => panic!("propose failed: {errors:?}"),
        }

        // 3. The reply is the balance the deployed `revVault.getBalance` put on its reply channel —
        //    read from a block and carried back over CapTP.
        //
        //    The value is the genesis wallet's balance **less the phlo this very deploy spent**
        //    (1_000_000_000_000 − 1_000_000). That is a stronger check than "a number arrived": the
        //    deploy paid for itself out of the vault it was reading, so the assertion pins the whole
        //    round trip rather than its shape.
        //
        //    **Bounded**, because "no block was produced" has to be a failure with the reason in it
        //    rather than a job that sits here until its timeout: the deploy lands only if a block
        //    carries it, and that is the node's half of this test.
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            client.recv_message(),
        )
        .await
        .expect(
            "the bridge should answer within 120s — a delivered call becomes a deploy and a block",
        )
        .expect("read the reply")
        .expect("a reply, not a closed connection");
        let delivered = Deliver::from_syrup(&reply).expect("a delivery");
        assert_eq!(
            delivered.args[0],
            Value::Symbol("fulfill".into()),
            "the bridge refused the delivery: {:?}",
            delivered.args.get(1)
        );
        assert_eq!(
            delivered.args[1],
            Value::Int(999_999_000_000i64.into()),
            "the balance should be the genesis wallet's less this deploy's phlo"
        );

        drop(client);
        node.shutdown();
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fetch an object by swiss number from the peer's bootstrap, returning the descriptor to address
/// it by.
async fn fetch(client: &mut Session, swiss: &[u8]) -> Desc {
    let request = Deliver {
        to: Desc::Export(0u64.into()),
        args: vec![Value::Symbol("fetch".into()), Value::Bytes(swiss.to_vec())],
        answer_pos: None,
        resolve_me_desc: Some(Desc::ImportObject(0u64.into())),
    };
    client
        .send_message(&request.to_syrup())
        .await
        .expect("send the fetch");
    let reply = client
        .recv_message()
        .await
        .expect("read the fetch reply")
        .expect("a reply");
    let delivered = Deliver::from_syrup(&reply).expect("a delivery");
    assert_eq!(delivered.args[0], Value::Symbol("fulfill".into()));
    match Desc::from_syrup(&delivered.args[1]).expect("a descriptor") {
        Desc::ImportObject(position) => Desc::Export(position),
        other => panic!("expected a descriptor for the object, got {other:?}"),
    }
}
