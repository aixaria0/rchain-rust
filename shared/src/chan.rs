//! **The send whose failure nothing saw** (C254's E6b).
//!
//! `let _ = tx.send(v)` is the shape of a *decision* — "the receiver may already be gone, and that is
//! fine" — and it reads exactly like a dropped error. On a shutdown path the decision is usually right:
//! the node is going down and the consumer left first. The problem is that a node that dropped work
//! during a shutdown and a node that shut down cleanly produce the same evidence, which is nothing.
//!
//! This module is the door those sends go through instead. The decision is kept — the value is still
//! discarded — but the event is counted, so the two cases can be told apart on a running node.
//!
//! **A door per shape of sender** — `mpsc::Sender` blocking and non-blocking, `mpsc::UnboundedSender`,
//! `oneshot::Sender`, `watch::Sender`, and [`best_effort`] for the sinks these do not cover (a
//! `futures` sink, a connection). One of them carries a distinction the raw shape erases: `try_send`
//! fails *either* because the receiver is gone *or* because the buffer is full, and only the first is
//! this module's business. A full buffer drops the value too, but the peer is still there and calling
//! that "gone" would make the counter lie, so [`try_send`] counts `Closed` and passes `Full` through as
//! a plain `false`. `best_effort` is the odd one out on purpose: it counts nothing, because its failures
//! have no receiver to count, and it earns its place by *naming the decision* instead of hiding it.
//!
//! **The silent shape is forbidden by the gate, not by a lint**, and that is a deliberate choice: the
//! crate-wide `clippy::let_underscore_must_use` catches every `let _ = <must_use>` in the tree, which in
//! tests alone means `let _ = thread::scope(..)` and `let _ = fs::remove_dir_all(..)` — churn on shapes
//! that have nothing to do with this. `tools/audit-type-system.sh` carries a hard `discard` class
//! instead: it is scoped to production code (the stripper drops `#[cfg(test)]` blocks), it matches the
//! send shape and nothing else, and it has a probe because its steady state is zero sites.
//!
//! **This is the source half, and it says so.** The count is process-wide and readable in process; the
//! surface beside `poisonRecoveries` on `/api/status` is the next increment, on the same reasoning
//! `rspace`'s lock module recorded when its own counter landed (C249's F-U9-03). Until that lands,
//! "counted but not yet published" is the honest description of what this is.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{mpsc, oneshot};

/// How many sends this process made and decided not to act on, and which did not arrive.
///
/// Process-wide because the far ends are: the consumer that dropped may be in any task. Monotone, so a
/// reader can compare two observations rather than race one.
static UNDELIVERED_SENDS: AtomicU64 = AtomicU64::new(0);

/// The number of sends this process has made that never arrived and were not acted on.
pub fn undelivered_sends() -> u64 {
    UNDELIVERED_SENDS.load(Ordering::Relaxed)
}

fn note_undelivered() {
    UNDELIVERED_SENDS.fetch_add(1, Ordering::Relaxed);
}

/// Blocking send, counting a receiver that had already gone. Returns whether it was delivered.
pub async fn send<T>(tx: &mpsc::Sender<T>, value: T) -> bool {
    match tx.send(value).await {
        Ok(()) => true,
        Err(_) => {
            note_undelivered();
            false
        }
    }
}

/// Non-blocking send, counting a receiver that had already gone. Returns whether it was delivered.
///
/// `false` covers **both** reasons a `try_send` can fail; only the closed case moves the counter. See
/// the module docs — a full buffer is not a gone receiver, and conflating them is how a counter starts
/// reporting something other than what it says.
pub fn try_send<T>(tx: &mpsc::Sender<T>, value: T) -> bool {
    match tx.try_send(value) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => false,
        Err(mpsc::error::TrySendError::Closed(_)) => {
            note_undelivered();
            false
        }
    }
}

/// Send on an unbounded channel, counting a receiver that had already gone.
pub fn unbounded_send<T>(tx: &mpsc::UnboundedSender<T>, value: T) -> bool {
    match tx.send(value) {
        Ok(()) => true,
        Err(_) => {
            note_undelivered();
            false
        }
    }
}

/// Send one value on a oneshot, counting a receiver that had already gone.
///
/// Takes the sender by value because `oneshot::Sender::send` consumes it — a oneshot sends at most once,
/// so there is nothing left to send with afterwards.
pub fn oneshot_send<T>(tx: oneshot::Sender<T>, value: T) -> bool {
    match tx.send(value) {
        Ok(()) => true,
        Err(_) => {
            note_undelivered();
            false
        }
    }
}

/// **Await a send whose outcome cannot be acted on, and say so in the name.**
///
/// The door for the shapes the typed ones do not cover — a `futures` `Sink`, a connection. A reader of
/// `best_effort(...)` learns the decision at the call site, where `let _ = something.send(..)` taught
/// them nothing, which is the whole of what C254's E6b asks for: the *decision* is fine, the *silence*
/// is not.
///
/// **It counts rather than erases, and that is not a style choice.** The first draft was `f.await.ok()`
/// — and the `silent` class in `tools/audit-type-system.sh` rejected it, correctly: that class exists to
/// stop "an I/O call's error erased into an absence", and a helper whose entire point is that erasure is
/// the worst place to make an exception, because every caller would inherit the silence the class was
/// written for. The explicit arm records the event through the same counter as the typed doors, so the
/// decision is visible in the one place an operator can see it.
pub async fn best_effort<F, T, E>(f: F) -> Option<T>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    match f.await {
        Ok(value) => Some(value),
        Err(_) => {
            note_undelivered();
            None
        }
    }
}

/// Publish on a `watch`, counting the case where nobody is watching any more.
///
/// A fifth shape, and the one whose failure is easiest to mistake for normal operation: a watch send
/// fails only when **every** receiver has dropped, which is exactly what an orderly shutdown looks like
/// — and also what a node that lost its supervisor looks like.
pub fn watch_send<T>(tx: &tokio::sync::watch::Sender<T>, value: T) -> bool {
    match tx.send(value) {
        Ok(()) => true,
        Err(_) => {
            note_undelivered();
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter is **process-wide**, so two of these tests running at once would see each other's
    /// increments — and the assertions below are exact rather than `>`, because "the full-buffer send
    /// must not move it" is a claim about a value and not about a direction. This serialises them;
    /// nothing outside this module touches the counter.
    static COUNTER_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        COUNTER_TESTS
            .lock()
            .expect("no counter test panics while holding this")
    }

    /// **The old shape, kept in the tree as the control.**
    ///
    /// This is what the sites looked like, and the test below runs it beside the new door so the
    /// difference is a measurement rather than a claim. The `allow` is here because the shape is the
    /// point of the function and `restriction` lints are allow-by-default anyway — the ban that matters
    /// is the `discard` class in `tools/audit-type-system.sh`, which scans production code and would
    /// reject this line outside a test block.
    #[allow(clippy::let_underscore_must_use)]
    fn the_old_shape(tx: &mpsc::Sender<u8>) {
        let _ = tx.try_send(1);
    }

    /// **A send to a gone receiver was silent and is now counted** (C254's E6b).
    ///
    /// The falsifier is the pair: the old shape moves the counter **not at all** while dropping the
    /// value, and the new door moves it once for the same event. On the pre-fix tree every site was the
    /// first, so a node that lost work during a shutdown reported exactly what a node that lost nothing
    /// reported.
    #[test]
    fn a_send_to_a_gone_receiver_moves_the_counter_and_the_old_shape_does_not() {
        let _guard = exclusive();
        let (tx, rx) = mpsc::channel::<u8>(1);
        drop(rx);

        let before = undelivered_sends();
        the_old_shape(&tx);
        assert_eq!(
            undelivered_sends(),
            before,
            "the old shape drops the value and says nothing — this is the defect, demonstrated"
        );

        assert!(
            !try_send(&tx, 2),
            "the receiver is gone, so the value is dropped"
        );
        assert_eq!(undelivered_sends(), before + 1, "…and the door counts it");
    }

    /// **A full buffer is not a gone receiver.** The distinction the raw `let _ = tx.try_send(v)` erased,
    /// and the reason the non-blocking door does not count every failure it sees: the value is dropped
    /// in both cases, but only one of them means nobody is listening any more.
    #[test]
    fn a_full_buffer_drops_the_value_without_counting_a_receiver_that_still_listens() {
        let _guard = exclusive();
        let (tx, _rx) = mpsc::channel::<u8>(1);
        assert!(try_send(&tx, 1), "the first send fits");

        let before = undelivered_sends();
        assert!(!try_send(&tx, 2), "the buffer is full");
        assert_eq!(
            undelivered_sends(),
            before,
            "full is not gone — counting it would make the counter mean something else"
        );
    }

    /// The three other doors count the same event, so a site does not have to change shape to be counted.
    #[tokio::test]
    async fn the_async_unbounded_and_oneshot_doors_all_count() {
        let _guard = exclusive();
        let (tx, rx) = mpsc::channel::<u8>(1);
        drop(rx);
        let before = undelivered_sends();
        assert!(!send(&tx, 1).await);
        assert_eq!(undelivered_sends(), before + 1);

        let (utx, urx) = mpsc::unbounded_channel::<u8>();
        drop(urx);
        assert!(!unbounded_send(&utx, 1));
        assert_eq!(undelivered_sends(), before + 2);

        let (otx, orx) = oneshot::channel::<u8>();
        drop(orx);
        assert!(!oneshot_send(otx, 1));
        assert_eq!(undelivered_sends(), before + 3);

        let (wtx, wrx) = tokio::sync::watch::channel(0u8);
        drop(wrx);
        assert!(!watch_send(&wtx, 1));
        assert_eq!(undelivered_sends(), before + 4);
    }

    /// And the ordinary path is the ordinary result: a live receiver takes the value and nothing is
    /// counted, so the counter is a measure of the fault and not of the traffic.
    #[tokio::test]
    async fn a_delivered_send_is_not_counted() {
        let _guard = exclusive();
        let (tx, mut rx) = mpsc::channel::<u8>(1);
        let before = undelivered_sends();
        assert!(send(&tx, 7).await);
        assert_eq!(rx.recv().await, Some(7));
        assert_eq!(undelivered_sends(), before);
    }
}
