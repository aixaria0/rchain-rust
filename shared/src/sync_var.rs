//! A blocking synchronized variable (port of `shared/SyncVarOps.scala`).
//!
//! Scala's `scala.concurrent.SyncVar` (an empty-or-full cell with blocking `take`/`put`) is
//! simplified to a `Mutex<Option<A>>` + `Condvar`.

use crate::lock::{cwait, mlock};
use std::sync::{Condvar, Mutex};

/// A blocking cell that is either empty or holds a value (port of `SyncVar`).
#[derive(Debug)]
pub struct SyncVar<A> {
    state: Mutex<Option<A>>,
    ready: Condvar,
}

impl<A> SyncVar<A> {
    /// A cell pre-filled with `a` (port of `SyncVarOps.create`).
    pub fn create(a: A) -> Self {
        SyncVar {
            state: Mutex::new(Some(a)),
            ready: Condvar::new(),
        }
    }

    /// Take the value, apply `f`, and put the result back (port of `RichSyncVar.update`).
    pub fn update(&self, f: impl FnOnce(A) -> A) {
        let curr = self.take();
        self.put(f(curr));
    }

    /// Block until non-empty, then return and remove the value.
    pub fn take(&self) -> A {
        let mut guard = mlock(&self.state);
        loop {
            if let Some(a) = guard.take() {
                self.ready.notify_one();
                return a;
            }
            guard = cwait(&self.ready, guard);
        }
    }

    /// Block until empty, then set the value.
    pub fn put(&self, a: A) {
        let mut guard = mlock(&self.state);
        while guard.is_some() {
            guard = cwait(&self.ready, guard);
        }
        *guard = Some(a);
        self.ready.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// Poison a mutex the way the real code would: panic while a guard is held.
    fn poison<T: Send>(mutex: &Mutex<T>) {
        let _ = thread::scope(|s| {
            s.spawn(|| {
                let _guard = mutex.lock().expect("unpoisoned");
                panic!("while holding the lock");
            })
            .join()
        });
    }

    /// **A poison recovered inside a `SyncVar` must be counted** (`C253` E2).
    ///
    /// Falsifier: on the pre-fix helper — `mutex.lock().unwrap_or_else(|p| p.into_inner())` — the guard
    /// still comes back but the process counter does not move, so the second assertion fails. That is
    /// the defect the finding names: an operator could read `poisonRecoveries: 0` on a node that had
    /// recovered from a panic in this very lock, and a `SyncVar` is on the consensus path.
    ///
    /// The first assertion is not decoration: it pins that the counter's non-movement is a *recovery*
    /// being invisible rather than a lock that was never poisoned.
    #[test]
    fn a_poison_recovered_inside_a_sync_var_is_counted() {
        let var = SyncVar::create(1u8);
        poison(&var.state);
        assert!(
            var.state.lock().is_err(),
            "the cell's mutex really is poisoned"
        );

        let before = crate::lock::poison_recoveries();
        assert_eq!(
            var.take(),
            1,
            "the value survived — the recovery is deliberate"
        );
        assert!(
            crate::lock::poison_recoveries() > before,
            "…and invisible no longer ({before} -> {})",
            crate::lock::poison_recoveries()
        );
    }

    #[test]
    fn create_put_and_take_round_trip() {
        let var = SyncVar::create(1);
        assert_eq!(var.take(), 1);
        var.put(2);
        assert_eq!(var.take(), 2);
    }

    #[test]
    fn update_applies_the_function() {
        let var = SyncVar::create(1);
        var.update(|x| x + 1);
        assert_eq!(var.take(), 2);
    }

    #[test]
    fn take_blocks_until_put() {
        let var = Arc::new(SyncVar::<i32>::create(0));
        var.take(); // now empty

        let producer = Arc::clone(&var);
        let handle = thread::spawn(move || {
            producer.put(42);
        });

        assert_eq!(var.take(), 42);
        handle.join().unwrap();
    }
}
