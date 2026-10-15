//! The read path's install leg, counted (issue #364).
//!
//! A read forks a runtime and resets it to the last finalized block's state. `RhoRuntime::reset`
//! re-installs every system process — the reset replaces the hot store, and the installs are not
//! checkpointed, so they have to be re-introduced. But [`build_runtime_core`](crate::runtime) has
//! *already* installed once, into the store the reset then discards. So the install runs **twice per
//! read**, and the first pass's warmed read cache is thrown away with the store.
//!
//! This module is what makes that a number on a node rather than a claim about the source.
//!
//! It pairs with `casper::runtime_manager::read_path_census`, which times `RhoRuntime::reset` as a
//! whole. `casper`'s `reset` stage minus this module's `reset_installs` is the space reset — the
//! split is available without changing `reset`'s signature or its behaviour, which is the point of
//! counting in statics.
//!
//! Statics rather than a handle, for the same reason as the other censuses in this tree: the runtime
//! has no logging handle on this path. **Relaxed ordering throughout** — a progress report, not a
//! barrier. Nothing here changes scheduling, ordering or results.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Every install pass, wherever it is called from — `build_runtime_core`, `RhoRuntime::reset`,
/// `ReplayRhoRuntime::reset`. Two per read is the number to watch for.
static INSTALL_CALLS: AtomicU64 = AtomicU64::new(0);
static INSTALL_TOTAL_NS: AtomicU64 = AtomicU64::new(0);
static INSTALL_MAX_NS: AtomicU64 = AtomicU64::new(0);
static INSTALLED_PROCESSES: AtomicU64 = AtomicU64::new(0);

/// The install inside `RhoRuntime::reset` only — the one leg of `reset` that this module exists to
/// separate from the space reset beside it.
static RESET_INSTALL_CALLS: AtomicU64 = AtomicU64::new(0);
static RESET_INSTALL_TOTAL_NS: AtomicU64 = AtomicU64::new(0);
static RESET_INSTALL_MAX_NS: AtomicU64 = AtomicU64::new(0);

fn ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// A clock, where there is one.
///
/// **`std::time::Instant::now()` panics on `wasm32-unknown-unknown`**, and the reducer is built for
/// that target and installs the system processes on the way up — so a clock read at a call site is a
/// panicking reducer, not a slow one (measured: the five `wasm_reduce` tests fail at
/// `install_system_processes`). This type is the one place the target decides, so the call sites read
/// the same on both: `Clock::start()` then `elapsed()`, which is `Duration::ZERO` where there is no
/// clock. The counts stay exact there — they are integers, not clock readings.
#[cfg(not(target_arch = "wasm32"))]
pub struct Clock(Option<std::time::Instant>);

#[cfg(not(target_arch = "wasm32"))]
impl Clock {
    pub fn start() -> Self {
        Clock(Some(std::time::Instant::now()))
    }

    pub fn elapsed(&self) -> Duration {
        self.0.map(|started| started.elapsed()).unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct Clock;

#[cfg(target_arch = "wasm32")]
impl Clock {
    pub fn start() -> Self {
        Clock
    }

    pub fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
}

/// One install pass over `defs` definitions.
pub fn record_install(defs: usize, duration: Duration) {
    let n = ns(duration);
    INSTALL_CALLS.fetch_add(1, Ordering::Relaxed);
    INSTALL_TOTAL_NS.fetch_add(n, Ordering::Relaxed);
    INSTALL_MAX_NS.fetch_max(n, Ordering::Relaxed);
    INSTALLED_PROCESSES.fetch_add(u64::try_from(defs).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// The install leg of `RhoRuntime::reset`.
pub fn record_reset_install(duration: Duration) {
    let n = ns(duration);
    RESET_INSTALL_CALLS.fetch_add(1, Ordering::Relaxed);
    RESET_INSTALL_TOTAL_NS.fetch_add(n, Ordering::Relaxed);
    RESET_INSTALL_MAX_NS.fetch_max(n, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LegSnapshot {
    pub calls: u64,
    pub total_ns: u64,
    pub max_ns: u64,
    /// Definitions installed across all passes. Only meaningful for the all-installs leg.
    pub installed_processes: u64,
}

impl LegSnapshot {
    pub fn mean_ns(&self) -> Option<u64> {
        (self.calls > 0).then(|| self.total_ns / self.calls)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Every install pass (two per read is the expected shape).
    pub installs: LegSnapshot,
    /// The install leg inside `RhoRuntime::reset`.
    pub reset_installs: LegSnapshot,
}

impl Snapshot {
    /// One line for a log or a captured run.
    pub fn summary(&self) -> String {
        format!(
            "installs: {} passes / {} ns mean (max {} ns), {} definitions; reset install leg: {} / {} ns mean (max {} ns)",
            self.installs.calls,
            self.installs.mean_ns().unwrap_or(0),
            self.installs.max_ns,
            self.installs.installed_processes,
            self.reset_installs.calls,
            self.reset_installs.mean_ns().unwrap_or(0),
            self.reset_installs.max_ns
        )
    }
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        installs: LegSnapshot {
            calls: INSTALL_CALLS.load(Ordering::Relaxed),
            total_ns: INSTALL_TOTAL_NS.load(Ordering::Relaxed),
            max_ns: INSTALL_MAX_NS.load(Ordering::Relaxed),
            installed_processes: INSTALLED_PROCESSES.load(Ordering::Relaxed),
        },
        reset_installs: LegSnapshot {
            calls: RESET_INSTALL_CALLS.load(Ordering::Relaxed),
            total_ns: RESET_INSTALL_TOTAL_NS.load(Ordering::Relaxed),
            max_ns: RESET_INSTALL_MAX_NS.load(Ordering::Relaxed),
            installed_processes: 0,
        },
    }
}

pub fn summary() -> String {
    snapshot().summary()
}
