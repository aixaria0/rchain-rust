//! A standalone `tcp-testing-only` OCapN peer, for the OCapN conformance suite.
//!
//! Usage: `ocapn-tcp-testing [host:port]` (default `127.0.0.1:22045`). It prints the locator to hand
//! to `test_runner.py`, then serves the suite's fixture objects on every inbound session.
//!
//! This is a test harness, not a node feature: the transport it speaks is the suite's own
//! explicitly-insecure one, and it exits only when killed.

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::Rng;

use rchain_ocapn::conn::{Identity, Session};
use rchain_ocapn::fixtures;
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:22045".to_string());
    let listener = TcpTestingOnly::bind(&addr).await?;
    let local = listener.local_addr()?;
    let host = local.ip().to_string();

    eprintln!("ocapn-tcp-testing listening on {local}");
    eprintln!(
        "python3 test_runner.py 'ocapn://rnode-ocapn.tcp-testing-only?host={host}&port={}' -v",
        local.port()
    );

    loop {
        let conn = listener.accept_incoming_connection().await?;
        let location = PeerLocator {
            designator: "rnode-ocapn".to_string(),
            transport: "tcp-testing-only".to_string(),
            hints: BTreeMap::from([
                ("host".to_string(), host.clone()),
                ("port".to_string(), local.port().to_string()),
            ]),
        };
        tokio::spawn(async move {
            // A fresh session key per session; OCapN never reuses one.
            let mut seed = [0u8; 32];
            rand::rng().fill_bytes(&mut seed);
            let identity = match Identity::from_seed(seed, location) {
                Ok(identity) => identity,
                Err(e) => {
                    eprintln!("session key: {e}");
                    return;
                }
            };
            let bootstrap = Arc::new(fixtures::conformance_bootstrap());
            match Session::accept(conn, &identity, bootstrap).await {
                Ok(mut session) => {
                    if let Err(e) = session.run().await {
                        eprintln!("session ended: {e}");
                    }
                }
                Err(e) => eprintln!("handshake refused: {e}"),
            }
        });
    }
}
