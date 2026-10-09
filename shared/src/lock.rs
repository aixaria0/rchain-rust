//! Poison-aware lock accessors.
//!
//! The engine uses `std::sync::RwLock`/`Mutex` for shared mutable state. A lock is only poisoned if
//! a panic occurred while it was held; these accessors recover the guard via `PoisonError::into_inner`
//! instead of panicking, making the poison recovery explicit and total (per `TYPE-SYSTEM.md` §3.2).
//!
//! **Recovering is deliberate; recovering *invisibly* was not.** A poison means a panic happened while
//! shared state was held, which is a serious event for a node — and until 2026-10-09 nothing recorded
//! it: no log, no counter, no surface, so an operator could not tell a node that had taken a panic
//! inside a store lock from one that had not. The failure-mode HAZOP named that as one of only two
//! genuinely silent paths in the node (`C249`'s F-U9-03). This counter is the **source half** of that
//! finding: the event is now recorded.
//!
//! # Two shapes, one mechanism — why the trait exists (`C253` E2)
//!
//! The accessors below were the original mechanism, and they are the right one **when the lock is a
//! place you can name**: `mlock(&self.overlay)` borrows a field, and the guard's lifetime is the
//! field's. But most of the codebase's lock sites are not like that. They read
//!
//! ```text
//! self.writermlock(self.writer).lock().unwrap_or_else(|p| p.into_inner())
//! ```
//!
//! — the receiver is an *expression* (`self.writermlock(..)` returns the `&Mutex`), so `mlock(&…)`
//! would borrow a temporary and the guard would dangle. That is not a stylistic preference: an
//! attempt to mechanically rewrite these sites into the accessor form produced a broken receiver at
//! 30 files (`self.writermlock(self.writer)` — the accessor's call swallowed by the very expression
//! it was meant to wrap). The fix landed here instead: [`Unpoison::unpoison`] is a method on the
//! **`LockResult` itself**, so the receiver is never touched and the rewrite is a same-shaped
//! substitution of the tail, `…unwrap_or_else(|p| p.into_inner())` → `….unpoison()`.
//!
//! Both shapes count. The accessors delegate to the trait, so there is exactly one place a recovery
//! is recorded, and `tools/audit-type-system.sh`'s hard `poison` class forbids the raw std tail —
//! the un-counted recovery is what the class exists to make unrepresentable.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{
    Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

/// How many times this process has recovered a poisoned lock.
///
/// Process-wide because the poisoning is: the panic happened in whichever thread held the guard.
/// Monotone, so a reader can compare two observations rather than race one.
static POISON_RECOVERIES: AtomicU64 = AtomicU64::new(0);

/// The number of poisoned-lock recoveries this process has performed.
pub fn poison_recoveries() -> u64 {
    POISON_RECOVERIES.load(Ordering::Relaxed)
}

fn note_poison() {
    POISON_RECOVERIES.fetch_add(1, Ordering::Relaxed);
}

/// Recover a lock guard from a poisoned acquisition **and count the recovery**.
///
/// Implemented on `LockResult<T>` — `Result<T, PoisonError<T>>`, the return type of both
/// `Mutex::lock` and `RwLock::read`/`write` — deliberately, so a call site needs no change to its
/// receiver. See the module docs for why that matters.
pub trait Unpoison<T> {
    /// Take the guard; if the lock was poisoned, record the recovery and take it anyway.
    fn unpoison(self) -> T;
}

impl<T> Unpoison<T> for Result<T, PoisonError<T>> {
    fn unpoison(self) -> T {
        match self {
            Ok(guard) => guard,
            Err(poisoned) => {
                note_poison();
                poisoned.into_inner()
            }
        }
    }
}

/// Acquire a read guard, recovering from poison **and counting the recovery**.
pub fn rlock<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unpoison()
}

/// Acquire a write guard, recovering from poison **and counting the recovery**.
pub fn wlock<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unpoison()
}

/// Acquire a mutex guard, recovering from poison **and counting the recovery**.
pub fn mlock<T>(l: &Mutex<T>) -> MutexGuard<'_, T> {
    l.lock().unpoison()
}

/// Wait on a condition variable, recovering from poison **and counting the recovery**.
///
/// A condvar wait has the same shape as an acquisition — it takes a guard and can hand back the
/// poison if the waiter's predecessor panicked — so it recovers through the same door. Named rather
/// than inlined so the raw `cv.wait(guard).unwrap_or_else(..)` tail stays greppable-and-banned.
pub fn cwait<'a, T>(cv: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(guard).unpoison()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Poison a `Mutex` the way the real code would: panic while a guard is held.
    fn poison(mutex: &Mutex<u8>) {
        let _ = std::thread::scope(|s| {
            s.spawn(|| {
                let _guard = mutex.lock().expect("unpoisoned");
                panic!("while holding the lock");
            })
            .join()
        });
    }

    /// Poison an `RwLock` by panicking while a write guard is held.
    fn poison_rw(lock: &RwLock<u8>) {
        let _ = std::thread::scope(|s| {
            s.spawn(|| {
                let _guard = lock.write().expect("unpoisoned");
                panic!("while holding the write lock");
            })
            .join()
        });
    }

    /// A panic while the lock is held poisons it; `std`'s own accessors then return `Err` and
    /// `unwrap` would panic *again*, turning a recovered-from fault into a crash loop. These
    /// accessors recover the guard instead (TYPE-SYSTEM.md §3.2: no silent partiality, and no
    /// gratuitous panic either) — the data is still there, so the operation is total.
    #[test]
    fn a_poisoned_lock_still_yields_its_guard() {
        let mutex = Mutex::new(7u8);
        poison(&mutex);
        assert!(mutex.lock().is_err(), "the mutex really is poisoned");
        assert_eq!(*mlock(&mutex), 7, "the value survived the poisoning");

        let rw = RwLock::new(9u8);
        poison_rw(&rw);
        assert!(rw.read().is_err(), "the rwlock really is poisoned");
        assert_eq!(*rlock(&rw), 9);
        assert_eq!(*wlock(&rw), 9);
    }

    /// **A poisoned recovery must not be silent.**
    ///
    /// The guard still comes back — that is the module's deliberate decision, pinned by the test above —
    /// but the *event* is now counted, so a node that has taken a panic inside a store lock can be told
    /// from one that has not (C249's F-U9-03; the two silent paths the failure-mode HAZOP found).
    ///
    /// Falsifier, both forms. On the old accessors (`unwrap_or_else(PoisonError::into_inner)`) there was
    /// nothing to observe: no counter existed and no accessor exposed one, so this test could not be
    /// written at all — which is exactly what "silent" meant. Post-fix the count moves once per
    /// recovery, on each of the three accessors.
    ///
    /// The assertions are `>` rather than `==` because the counter is **process-wide** and libtest runs
    /// this module's tests concurrently, so another test's poisoning is a legitimate concurrent
    /// increment. That a recovery moves the count is the property; its exact value is shared state.
    #[test]
    fn a_poisoned_recovery_is_counted() {
        let before = poison_recoveries();

        // **Every guard here is a temporary, and that is load-bearing.** A named binding would hold the
        // read guard across the `wlock` below, and `std::sync::RwLock` is not reentrant — upgrading
        // deadlocks the thread. The sibling test above uses the same form; this note is here because the
        // first draft of *this* test named its guards and hung for fifteen minutes.
        let mutex = Mutex::new(1u8);
        poison(&mutex);
        assert_eq!(*mlock(&mutex), 1, "the guard still comes back");
        assert!(
            poison_recoveries() > before,
            "…and the recovery is counted ({before} -> {})",
            poison_recoveries()
        );

        let after_mutex = poison_recoveries();
        let rw = RwLock::new(2u8);
        poison_rw(&rw);
        assert_eq!(*rlock(&rw), 2, "the read guard still comes back");
        assert!(
            poison_recoveries() > after_mutex,
            "…and the read recovery is counted"
        );

        let after_read = poison_recoveries();
        assert_eq!(*wlock(&rw), 2, "the write guard still comes back");
        assert!(
            poison_recoveries() > after_read,
            "…and the write recovery is counted"
        );
    }

    /// **The tail-shape the 136 raw sites carry counts exactly as the accessor does.**
    ///
    /// `Result::unpoison` is the mechanism the converted sites use, and a recovery through it must be
    /// indistinguishable from one through `mlock` — same door, same counter. This is the property
    /// `C253` E2 turns on: before the conversion, `l.lock().unwrap_or_else(|p| p.into_inner())`
    /// recovered *without* moving the counter, so `poisonRecoveries` could read 0 on a node that had
    /// recovered from a panic. The raw form is gone from the tree (the hard `poison` class in
    /// `tools/audit-type-system.sh` keeps it gone); this pins that the replacement is counted.
    #[test]
    fn the_result_side_recovery_is_counted_too() {
        let mutex = Mutex::new(11u8);
        poison(&mutex);
        assert!(mutex.lock().is_err(), "the mutex really is poisoned");

        let before = poison_recoveries();
        assert_eq!(*mutex.lock().unpoison(), 11, "the guard still comes back");
        assert!(
            poison_recoveries() > before,
            "…and a recovery through the trait moves the same counter"
        );
    }

    /// `cwait` is total on the ordinary path: it returns once the condition variable is notified.
    ///
    /// Its *counting* is not asserted here, deliberately — proving it would mean waiting on a condvar
    /// whose mutex is poisoned, and whether `Condvar::wait` short-circuits on a poisoned mutex is a
    /// `std` implementation detail, not a guarantee. A test that depends on it either hangs or passes
    /// for the wrong reason. The property that matters (`cwait` recovers through `Unpoison`) is
    /// visible by construction: it is `cv.wait(guard).unpoison()` and nothing else.
    #[test]
    fn cwait_returns_on_a_notification() {
        let pair = std::sync::Arc::new((Mutex::new(0u8), Condvar::new()));
        // **The mutex is held across the spawn, and that is the whole difference between a test and a
        // race.** The first version spawned the notifier and *then* waited: if the notifier won the
        // scheduler — which it does on a loaded runner — `notify_one` arrived before the wait had begun
        // and was, by `Condvar`'s own semantics, simply lost, so `cwait` waited for ever. It took a
        // 45-minute CI job down with it (the job was cancelled with this crate's test binary still
        // alive). Holding the lock here means the notifier cannot take it until `cwait` has released it
        // *inside* `wait`, so the ordering is a property of the program rather than of the scheduler.
        //
        // The lesson is not new: `chan`'s module docs state the same hazard for a `notify_one` that
        // arrives before its waiter, written a day before this test ignored it.
        let guard = mlock(&pair.0);
        let notifier = std::sync::Arc::clone(&pair);
        let handle = std::thread::spawn(move || {
            let _guard = mlock(&notifier.0);
            notifier.1.notify_one();
        });
        let guard = cwait(&pair.1, guard);
        assert_eq!(*guard, 0);
        drop(guard);
        handle.join().expect("the notifier returns");
    }

    /// The unpoisoned path is the ordinary one, and the guards are real: a write through `wlock` is
    /// visible to a later `rlock`, which is what every caller assumes.
    #[test]
    fn an_unpoisoned_lock_behaves_like_the_std_one() {
        let mutex = Mutex::new(1u8);
        *mlock(&mutex) += 1;
        assert_eq!(*mlock(&mutex), 2);

        // The trait, on the same unpoisoned path: no recovery, so the guard is just the guard.
        assert_eq!(*Mutex::new(5u8).lock().unpoison(), 5);

        let rw = RwLock::new(1u8);
        *wlock(&rw) += 41;
        assert_eq!(*rlock(&rw), 42);
    }
}
