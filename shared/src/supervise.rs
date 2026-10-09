//! **A detached task whose death is reported** (AUDIT C254).
//!
//! The audit's structural lens found that the node has no supervisor at all: every long-lived
//! background task is a bare `tokio::spawn` whose `JoinHandle` is dropped, so **nothing** can tell a
//! task that is running from one that died at startup. A panic reaches stderr through the default
//! panic hook and is at least visible; a plain early `return` — the loop that exits on its first
//! error, the future that completes when its channel closes — produces **no line anywhere**, and the
//! node then serves with that subsystem silently gone.
//!
//! This is the smallest mechanism that turns either death into a reported event: spawn the work, and
//! spawn one watcher that awaits its handle. The watcher is the whole of the supervisor — there is no
//! registry to keep in step, no `JoinSet` to own, and the call sites keep their shape
//! (`tokio::spawn(fut)` becomes `spawn_supervised("name", fut)`).
//!
//! **Both deaths are reported, and they are told apart.** A panic increments
//! [`tasks_panicked`] and prints the payload; a normal return increments [`tasks_exited`] and says so
//! at `warn`, because "this loop stopped" is the quieter and more dangerous of the two — the panic at
//! least announced itself once.
//!
//! **What this is not.** It does not restart anything, and it does not decide that a return is a
//! fault: some of the six tasks are *supposed* to end (a per-connection handshake finishes). It
//! reports, so that "the subsystem stopped" is a fact an operator can see instead of an absence they
//! have to infer. The counters reach `/api/status` beside `finalityStall`.
//!
//! The line goes to stderr rather than through [`crate::log`] because the supervisor is the one place
//! that runs **without** a logger in scope — the call sites that own one pass it to their own work,
//! and requiring it here would make the mechanism unusable at exactly the sites that need it.

use std::sync::atomic::{AtomicU64, Ordering};

/// Supervised tasks that have **panicked** since start (monotone).
static TASKS_PANICKED: AtomicU64 = AtomicU64::new(0);

/// Supervised tasks that have **returned** since start (monotone) — the deaths no panic hook sees.
static TASKS_EXITED: AtomicU64 = AtomicU64::new(0);

/// How many supervised tasks have panicked since this process started.
pub fn tasks_panicked() -> u64 {
    TASKS_PANICKED.load(Ordering::Relaxed)
}

/// How many supervised tasks have returned since this process started.
pub fn tasks_exited() -> u64 {
    TASKS_EXITED.load(Ordering::Relaxed)
}

/// Spawn `fut` and report its death — see the module doc for why a plain return is reported too.
///
/// The returned handle is the **watcher's**, and it is returned only so a caller that wants to await
/// the pair at shutdown can; the six sites in the tree discard it, which is what they did with the
/// bare `tokio::spawn` before.
pub fn spawn_supervised<F>(name: &'static str, fut: F) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(fut);
    tokio::spawn(async move {
        match handle.await {
            Ok(()) => {
                TASKS_EXITED.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "WARN  [task-supervisor] task `{name}` returned; whatever it was doing has stopped"
                );
            }
            Err(e) if e.is_panic() => {
                TASKS_PANICKED.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "ERROR [task-supervisor] task `{name}` PANICKED: {}",
                    panic_payload(&e.into_panic())
                );
            }
            // Cancelled: the runtime is shutting down, which is not a death worth counting.
            Err(_) => {}
        }
    })
}

/// The panic's message, when it carries one. `panic!` with a format string carries a `String`; with a
/// bare literal, a `&str`; anything else is a payload this crate cannot render, and saying *that* is
/// more useful than an empty message.
fn panic_payload(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<panic payload is not a string>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A task that panics is counted, and a task that returns is counted separately.**
    ///
    /// This is the falsifier for C254 in miniature: before it, neither event had a counter, so a
    /// probe could not ask whether a subsystem had died. The two counters are separate on purpose —
    /// a panic announces itself on stderr once, a return does not announce itself anywhere, and
    /// conflating them would lose the quieter one.
    ///
    /// The assertions are `>` against a captured reading rather than exact deltas: the counters are
    /// process-wide and libtest runs tests concurrently.
    #[tokio::test]
    async fn a_panicking_task_and_a_returning_task_are_counted_apart() {
        let panics_before = tasks_panicked();
        let exits_before = tasks_exited();

        // A task that returns normally — the death the panic hook cannot see.
        spawn_supervised("returns", async {}).await.ok();
        assert!(
            tasks_exited() > exits_before,
            "a task that returns is counted ({exits_before} -> {})",
            tasks_exited()
        );

        // A task that panics. Its payload is printed by the watcher; the assertion is on the count.
        let panics_mid = tasks_panicked();
        spawn_supervised("panics", async { panic!("the subsystem fell over") })
            .await
            .ok();
        assert!(
            tasks_panicked() > panics_mid,
            "a task that panics is counted ({panics_mid} -> {})",
            tasks_panicked()
        );
        assert!(
            tasks_panicked() > panics_before,
            "and it is a *panic*, not an exit — the two are different facts"
        );
    }

    /// The payload renderer keeps the message: a supervisor that reports `task X died` without the
    /// reason sends the operator back to a log that has no more than it does.
    #[test]
    fn the_panic_payload_keeps_its_message() {
        let from_str: Box<dyn std::any::Any + Send> = Box::new("a literal");
        assert_eq!(panic_payload(from_str.as_ref()), "a literal");
        let from_string: Box<dyn std::any::Any + Send> = Box::new(String::from("formatted 42"));
        assert_eq!(panic_payload(from_string.as_ref()), "formatted 42");
        let other: Box<dyn std::any::Any + Send> = Box::new(7u8);
        assert_eq!(
            panic_payload(other.as_ref()),
            "<panic payload is not a string>"
        );
    }
}
