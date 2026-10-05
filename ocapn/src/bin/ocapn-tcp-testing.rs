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

use rchain_ocapn::conn::Identity;
use rchain_ocapn::fixtures;
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::owner::{session_slot, SessionRegistry};
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:22045".to_string());
    let bound = TcpTestingOnly::bind(&addr).await?;
    // `local_addr` belongs to the concrete netlayer, not to the trait: take it before the Arc.
    let local = bound.local_addr()?;
    let listener: Arc<dyn Netlayer> = Arc::new(bound);
    let host = local.ip().to_string();
    // One registry for the process: the crossed-hello rule compares the sessions *this peer* has
    // with another, so it cannot be per-session state.
    let registry = Arc::new(SessionRegistry::default());
    // The exporter half of third-party handoffs, shared across sessions on purpose: a gift is
    // deposited on the gifter's connection and withdrawn on the receiver's.
    let handoffs = Arc::new(rchain_ocapn::handoff::Handoffs::default());

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
        let netlayer = listener.clone();
        let registry = registry.clone();
        let handoffs = handoffs.clone();
        let location = location.clone();
        tokio::spawn(async move {
            // A fresh session key per session; OCapN never reuses one.
            let mut seed = [0u8; 32];
            rand::rng().fill_bytes(&mut seed);
            let identity = match Identity::from_seed(seed, location.clone()) {
                Ok(identity) => identity,
                Err(e) => {
                    eprintln!("session key: {e}");
                    return;
                }
            };
            // The per-session objects that dial (the greeter and the enlivener) are built before the
            // session exists, because `accept` needs the bootstrap — so they find their session
            // through this slot, which is filled in below.
            let slot = session_slot();
            let mut bootstrap = fixtures::conformance_bootstrap_with(
                handoffs.clone(),
                slot.clone(),
                registry.clone(),
            );
            fixtures::publish_dialing_fixtures(
                &mut bootstrap,
                netlayer,
                location,
                registry.clone(),
                slot.clone(),
            );
            // **Book, then answer**: `accept_and_book` reads the peer's start-session, registers the
            // session (deciding any crossing) and only then writes ours. Answering first is a race
            // the handoff fixture caught — the peer speaks as soon as it reads our start-session, and
            // a receiver handed a sturdyref to this very peer would not find the session to reuse.
            let (handle, loop_, context, peer_location) =
                match rchain_ocapn::owner::accept_and_book(
                    conn,
                    &identity,
                    Arc::new(bootstrap),
                    &registry,
                )
                .await
                {
                    Ok(parts) => parts,
                    Err(reason) => {
                        eprintln!("session not served: {reason}");
                        return;
                    }
                };
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(context);
            if let Err(e) = loop_.run().await {
                eprintln!("session ended: {e}");
            }
            registry.forget(&peer_location, &handle.own_pi, handle.dialed);
        });
    }
}
