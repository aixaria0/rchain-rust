//! The node's stop path (AUDIT C144).
//!
//! Nothing installed a signal handler before this module, so the only stop was a `SIGKILL` after the
//! orchestrator's termination grace — and inside the container, where the node is PID 1 and the
//! kernel does not apply the default terminate disposition, even `SIGTERM` was delivered and
//! discarded. `docker stop`, systemd's `ExecStop` and a pod eviction all take that path; the register
//! measured it (`docker kill -s TERM` left the container `Running=true` twenty seconds later with no
//! log line, and a plain `docker stop` ended at `ExitCode=137`).
//!
//! The coordinator is **one `watch` channel** rather than a future handed to each server, and the
//! reason is the shape of the thing: a stop is a single event that six listeners must all observe.
//! [`shutdown_signal`] waits for the operator, [`stop_requested`] is what each listener awaits, and
//! [`crate::runtime::node_runtime::NodeProgram::serve`] drains behind them before returning.

use std::time::Duration;

use tokio::sync::watch;

/// How long the listeners are given to finish what they have in flight once the operator asks the
/// node to stop. Bounded on purpose: a wedged listener must not be able to hold the process past the
/// orchestrator's termination grace, which is the cost this module exists to stop paying.
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolves when the coordinator asks this listener to stop.
///
/// `wait_for`, not `changed`: it also answers immediately for a receiver created *after* the word
/// went out, so a listener that comes up late cannot miss the stop. A dropped sender resolves it too
/// — nothing is left that could ask — which is safe because the coordinator is held for as long as
/// the servers are.
pub async fn stop_requested(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stop| *stop).await;
}

/// Wait for the operator's stop: `SIGTERM` (what `docker stop`, systemd and a pod eviction send) or
/// `SIGINT` (Ctrl-C). Returns which one arrived, so the log line can name it.
pub async fn shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        // A failure here means the process's signal table cannot be written, which no handler in
        // this process can fix: fall back to Ctrl-C rather than panicking. No `.expect` — the
        // type-system gate's `panic` class scans this crate's production code, and a node that
        // cannot be *started* into a handler is a smaller fault than one that aborts.
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = tokio::signal::ctrl_c() => "SIGINT",
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                "SIGINT"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "SIGINT"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener's half of the stop path, in both directions: the future a server is handed must
    /// **not** resolve while nobody has asked it to stop, and must resolve as soon as somebody has.
    /// The first half is the one worth having — a `stop_requested` that resolved early would take
    /// every listener down the moment the node started serving.
    #[tokio::test]
    async fn stop_requested_waits_for_the_word_and_then_resolves() {
        let (tx, rx) = watch::channel(false);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), stop_requested(rx.clone()))
                .await
                .is_err(),
            "a listener must not stop before the coordinator asks it to"
        );
        tx.send(true).expect("the receiver is held");
        tokio::time::timeout(Duration::from_secs(1), stop_requested(rx))
            .await
            .expect("a listener must stop once the coordinator asks it to");
    }
}
