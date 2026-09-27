//! **AUDIT C144** — the node stops when it is asked to, and drains on the way out.
//!
//! Nothing installed a signal handler before the fix, so the only stop was a `SIGKILL` after the
//! orchestrator's termination grace. Inside the container the node is **PID 1**, where the kernel does
//! not apply the default terminate disposition, so `SIGTERM` — what `docker stop`, systemd's
//! `ExecStop` and a pod eviction all send — was delivered and discarded: the register measured a
//! container still `Running=true` twenty seconds after `docker kill -s TERM`, with no log line, and a
//! plain `docker stop` ending at `ExitCode=137`.
//!
//! **What is testable here, and what is not, stated rather than implied.** The OS signal itself is
//! the thin glue (`main.rs` → `runtime::shutdown::shutdown_signal`), and a test cannot raise `SIGTERM`
//! against its own process without killing the test runner — so what this file pins is the half the
//! signal is *wired to*: the stop path through `NodeProgram::serve`, the same watch channel the
//! handler flips. The end-to-end probe stays the register's own (`docker kill -s TERM` exiting
//! promptly), and it is the reason this half is worth pinning: a handler that flipped a channel
//! nothing listened to would leave the container exactly as stuck as before.

mod common;

use std::time::Duration;

use common::{deploy_conf, free_ports, start, temp_dir};

/// Poll until the node's HTTP API answers at all — the node is serving, not merely spawned.
async fn wait_for_status(client: &reqwest::Client, base: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(resp) = client.get(format!("{base}/api/status")).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the node never answered /api/status: {base}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The stop path, in both directions and in one boot.
///
/// The **control comes first and is the load-bearing half**: a node nobody has asked to stop must
/// keep serving, and `serve` must still be running. A `stop_requested` that resolved early — or a
/// sender dropped by the harness — would take every listener down at startup, and without this arm
/// the test would still pass, because a node that never served also "returns promptly when asked".
#[test]
fn a_node_that_is_asked_to_stop_returns_and_stops_answering() {
    common::test_runtime().block_on(async {
        let dir = temp_dir("shutdown");
        let ports = free_ports(5);
        let conf = deploy_conf(&dir, &ports);
        let mut node = start(&conf, ports[4], ports[0]).await;
        let base = format!("http://127.0.0.1:{}", ports[0]);
        let client = reqwest::Client::new();
        wait_for_status(&client, &base).await;

        assert!(
            tokio::time::timeout(Duration::from_secs(3), &mut node.handle)
                .await
                .is_err(),
            "the node must keep serving until somebody asks it to stop"
        );

        node.request_stop();
        let served = tokio::time::timeout(Duration::from_secs(30), node.handle)
            .await
            .expect("a node that was asked to stop must return rather than serve on (AUDIT C144)")
            .expect("the serve task must not panic");
        served.expect("a stop on the operator's word is not a server error");

        // The listeners really stopped: the port that answered a moment ago is closed. `serve`
        // returning is the node's own word; this is the socket's.
        assert!(
            client
                .get(format!("{base}/api/status"))
                .send()
                .await
                .is_err(),
            "the HTTP listener must be closed after the stop, not merely unreported"
        );

        let _ = std::fs::remove_dir_all(&dir);
    });
}
