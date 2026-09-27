//! **AUDIT C142** — a server that fails to bind is reported, and the report names the listener.
//!
//! Every listener the node starts is an accept loop, so it should only ever stop when something has
//! already gone wrong. `NodeProgram::serve` used to `join!` them and surface the errors with `??`
//! *after* the join returned — but four of the five never return, so a listener whose `bind` was
//! refused (its task ending with `Err` at once) left the join pending forever. The node then ran with
//! a dead component, logged nothing at all, and `main.rs`'s `Server error:` arm was unreachable in
//! practice. The finding demonstrated it in the shipped image: a second node started on a held
//! `--api-port-http` served nothing, but answered nothing else either — the port was the *first*
//! node's, so the health check passed off the wrong process.
//!
//! The falsifier holds one port, boots a node that is otherwise free to bind everywhere, and requires
//! the node's own task to complete with an `Err` naming that listener. On the pre-fix tree the task
//! never completes and the timeout below fires; that is the red this pins.
//!
//! Two listeners are driven, not one: the finding's own demonstration used the HTTP port, and the
//! claim to falsify is "the same shape on every listener" — so a second case takes a non-HTTP
//! listener, which the route-level tests could never reach.

mod common;

use std::time::Duration;

use common::{deploy_conf, free_ports, start, temp_dir};

/// Boot a node with `held_index`'s port already taken by this process, and return the message the
/// node's server task reports. 30 s is a bound on failure, not a schedule: with the fix the task ends
/// as soon as the refused `bind` returns.
async fn report_for_held_port(name: &str, held_index: usize) -> String {
    let dir = temp_dir(name);
    let ports = free_ports(5);
    // Held for the whole run: dropping this listener would let the node bind and the test would
    // measure nothing.
    let _held =
        std::net::TcpListener::bind(("127.0.0.1", ports[held_index])).expect("hold the port");
    let conf = deploy_conf(&dir, &ports);
    let node = start(&conf, ports[4], ports[0]).await;
    let outcome = tokio::time::timeout(Duration::from_secs(30), node.handle)
        .await
        .expect("the node's server task must finish once a listener cannot bind (AUDIT C142)")
        .expect("the server task must not panic");
    let err = outcome
        .expect_err("a node that cannot bind a listener must report it rather than serve the rest");
    let _ = std::fs::remove_dir_all(&dir);
    err
}

/// The finding's own demonstration: the HTTP port is the one an operator's health check depends on,
/// and losing it used to be silent.
#[test]
fn a_node_whose_http_port_is_taken_reports_the_listener_it_could_not_bind() {
    common::test_runtime().block_on(async {
        let err = report_for_held_port("listener-failure-http", 0).await;
        assert!(
            err.contains("HTTP listener"),
            "the report must name the listener that died — a bare \"a server failed\" leaves the \
             operator with five components and no way to tell which is missing: {err}"
        );
    });
}

/// A listener that is not HTTP, so the fix cannot be an artefact of the HTTP path. The internal gRPC
/// server carries propose + repl and binds loopback; a validator that loses it keeps producing blocks
/// but stops answering its own tooling.
#[test]
fn a_node_whose_internal_grpc_port_is_taken_reports_that_listener_too() {
    common::test_runtime().block_on(async {
        let err = report_for_held_port("listener-failure-grpc", 2).await;
        assert!(
            err.contains("internal gRPC listener"),
            "the select has to name the listener that stopped, whichever one it is: {err}"
        );
    });
}
