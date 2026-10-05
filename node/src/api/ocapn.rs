//! The node's OCapN listener (issue #249).
//!
//! OCapN is the object-capability network an Agoric vat speaks: a peer dials, the two sides
//! handshake over a netlayer, and the peer works against objects the node exports. This module is
//! the node's end of that — a listener that serves a fresh session per connection.
//!
//! **What it serves today** is the conformance fixtures, not the chain: the bridge that turns a
//! delivery into a signed deploy is the next unit. That is deliberate — it gives the listener a
//! falsifiable gate of its own (a client dials the node and gets a reply) before the chain is in
//! the loop.
//!
//! **The transport is `tcp-testing-only`**, which the OCapN project's own README flags as "HIGHLY
//! INSECURE — DO NOT USE IN PRODUCTION": plain TCP, no encryption, no authentication. The listener
//! is therefore off unless `api-server.ocapn-listen` names an address, and a node that is reachable
//! from anywhere it does not control should leave it unset.

use std::collections::BTreeMap;
use std::sync::Arc;

use rchain_ocapn::conn::{Identity, Session};
use rchain_ocapn::fixtures;
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;
use tokio::sync::watch;

use crate::runtime::shutdown::stop_requested;

/// Serve OCapN on `listen` (`host:port`) until the node is asked to stop.
///
/// `None` means the listener is not configured. The task is spawned either way — one that is simply
/// waiting on the stop word when there is nothing to serve — so the node's listener set stays
/// uniform and its `select!` does not need a second optional arm.
pub async fn serve_ocapn(
    listen: Option<String>,
    stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let Some(listen) = listen else {
        stop_requested(stop).await;
        return Ok(());
    };
    let listener = TcpTestingOnly::bind(&listen)
        .await
        .map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let location = PeerLocator {
        designator: "rnode".to_string(),
        transport: "tcp-testing-only".to_string(),
        hints: BTreeMap::from([
            ("host".to_string(), local.ip().to_string()),
            ("port".to_string(), local.port().to_string()),
        ]),
    };

    loop {
        let connection = tokio::select! {
            // The operator's word, and a dropped coordinator, both land here.
            _ = stop_requested(stop.clone()) => return Ok(()),
            accepted = listener.accept_incoming_connection() => accepted.map_err(|e| e.to_string())?,
        };
        let location = location.clone();
        tokio::spawn(async move {
            // A fresh session key per session, as OCapN requires.
            let Ok(identity) = Identity::fresh(location) else {
                return;
            };
            let bootstrap = Arc::new(fixtures::conformance_bootstrap());
            match Session::accept(connection, &identity, bootstrap).await {
                Ok(mut session) => {
                    // One session per connection; its end is this task's end.
                    let _ = session.run().await;
                }
                Err(_) => {
                    // A refused handshake has already been answered with `op:abort` where one was
                    // owed; there is nothing further to say on the wire.
                }
            }
        });
    }
}
