//! The read path's substrate counters (issue #364).
//!
//! A read forks a runtime and resets it to the last finalized block's state. The state
//! materialisation itself is O(1) — one root-node load (`RadixHistory::reset`) — so the question this
//! module answers is not *how deep* the state is but **what the per-request setup actually touches**:
//! how many history reads a read pays, how many of them reach the key-value store rather than an
//! in-memory node, and how long the space reset takes.
//!
//! These are **counts as much as times**, and that is deliberate. A count needs no wall clock and no
//! quiet box: "101 history reads per install pass, two passes per read" is a property of the build,
//! and it either shows up on a node or it does not. The times are here to rank the counts that do
//! dominate.
//!
//! Statics rather than a handle, for the same reason as the other censuses in this tree. **Relaxed
//! ordering throughout** — a progress report, not a barrier. Nothing here changes behaviour.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Every `*_from_history_store` call, across the three kinds (datum, continuation, joins).
static HISTORY_READS: AtomicU64 = AtomicU64::new(0);
/// The subset of those that had to reach the reader — i.e. that missed the per-store memo. A miss
/// count near the call count is what "the store's cache is cold on every request" looks like.
static HISTORY_READ_MISSES: AtomicU64 = AtomicU64::new(0);

/// `RadixTreeImpl::load_node` answered from the in-memory node cache.
static TRIE_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
/// `load_node` had to go to the store.
static TRIE_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
/// Every key-value read for a trie node, wherever it is initiated.
static TRIE_STORE_LOADS: AtomicU64 = AtomicU64::new(0);

static SPACE_RESET_CALLS: AtomicU64 = AtomicU64::new(0);
static SPACE_RESET_TOTAL_NS: AtomicU64 = AtomicU64::new(0);
static SPACE_RESET_MAX_NS: AtomicU64 = AtomicU64::new(0);

fn ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// A clock, where there is one — `std::time::Instant::now()` **panics on `wasm32-unknown-unknown`**,
/// and this crate is built for that target (the reducer). One cfg here rather than one at each call
/// site, and the counts beside it stay exact there: they are integers, not clock readings.
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

/// One history read, and whether it was served from the per-store memo.
pub fn record_history_read(was_cached: bool) {
    HISTORY_READS.fetch_add(1, Ordering::Relaxed);
    if !was_cached {
        HISTORY_READ_MISSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// One `load_node` answered from the node cache.
pub fn record_trie_cache_hit() {
    TRIE_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

/// One `load_node` that had to consult the store.
pub fn record_trie_cache_miss() {
    TRIE_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

/// One key-value read for a trie node.
pub fn record_trie_store_load() {
    TRIE_STORE_LOADS.fetch_add(1, Ordering::Relaxed);
}

/// One `ISpace::reset`.
pub fn record_space_reset(duration: Duration) {
    let n = ns(duration);
    SPACE_RESET_CALLS.fetch_add(1, Ordering::Relaxed);
    SPACE_RESET_TOTAL_NS.fetch_add(n, Ordering::Relaxed);
    SPACE_RESET_MAX_NS.fetch_max(n, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub history_reads: u64,
    pub history_read_misses: u64,
    pub trie_cache_hits: u64,
    pub trie_cache_misses: u64,
    pub trie_store_loads: u64,
    pub space_reset_calls: u64,
    pub space_reset_total_ns: u64,
    pub space_reset_max_ns: u64,
}

impl Snapshot {
    pub fn space_reset_mean_ns(&self) -> Option<u64> {
        (self.space_reset_calls > 0).then(|| self.space_reset_total_ns / self.space_reset_calls)
    }

    /// One line for a log or a captured run.
    pub fn summary(&self) -> String {
        format!(
            "substrate: {} history reads ({} misses); trie {} hits / {} misses / {} store loads; space reset {} calls, {} ns mean (max {} ns)",
            self.history_reads,
            self.history_read_misses,
            self.trie_cache_hits,
            self.trie_cache_misses,
            self.trie_store_loads,
            self.space_reset_calls,
            self.space_reset_mean_ns().unwrap_or(0),
            self.space_reset_max_ns
        )
    }
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        history_reads: HISTORY_READS.load(Ordering::Relaxed),
        history_read_misses: HISTORY_READ_MISSES.load(Ordering::Relaxed),
        trie_cache_hits: TRIE_CACHE_HITS.load(Ordering::Relaxed),
        trie_cache_misses: TRIE_CACHE_MISSES.load(Ordering::Relaxed),
        trie_store_loads: TRIE_STORE_LOADS.load(Ordering::Relaxed),
        space_reset_calls: SPACE_RESET_CALLS.load(Ordering::Relaxed),
        space_reset_total_ns: SPACE_RESET_TOTAL_NS.load(Ordering::Relaxed),
        space_reset_max_ns: SPACE_RESET_MAX_NS.load(Ordering::Relaxed),
    }
}

pub fn summary() -> String {
    snapshot().summary()
}
