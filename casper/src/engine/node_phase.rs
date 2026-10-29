//! **The node's phase, as launch decided it** (HAZOP U20/B47, RCA S7/getting-ready).
//!
//! Before this cell there was no phase at all: `NodeLaunch` chose genesis, recovery or reconnection
//! and then constructed `NodeRunning`, and every surface had to reconstruct "is this node serving?"
//! from a set of unrelated booleans. A syncing node, a node whose recovery had not started, and a
//! running node with a faulted pipeline looked different only because a reader happened to combine
//! the right fields.
//!
//! The phase is **set by launch and only by launch**; the disposition loops wait for
//! [`NodePhase::Running`] so a start-up re-offer cannot race a recovery into the live DAG, and a
//! propose that arrives while launch is still deciding waits for the decision
//! ([`wait_until_decided`]) instead of answering a healthy boot with a failure. This is not a
//! consensus fact and is not persisted: a restart begins at [`NodePhase::Starting`].
//!
//! **What it does not yet do, said rather than implied:** `Running` is set immediately before
//! `NodeRunning::new`, not after the start-up disposition audit completes, so a body re-offered at
//! start-up can still be in flight while the node first reports running. The audit's confirmation
//! (three sustained samples) is what withholds readiness for a real orphan; a constructor that
//! cannot build `Running` with an unsettled row remains owed (U19/B46). The cell is also **one per
//! process**, not one per shard: on a multi-shard node the first shard to reach the running engine
//! reports `Running` for all of them, so a node with one running and one syncing shard is
//! under-reported. Per-shard phase is owed with the other multi-shard readiness work.

use std::sync::Mutex;
use std::time::Duration;

use rchain_shared::lock::Unpoison;

/// Where the node is in its start-up lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodePhase {
    /// Launch has not yet chosen a path (genesis, sync, reconnection).
    Starting,
    /// The node is restoring the last finalized state; it serves no chain and proposes nothing.
    Syncing,
    /// Launch reached the running engine: the DAG is the node's served chain.
    Running,
    /// Launch refused to serve (a terminal sync failure, or a named launch failure).
    Blocked(String),
}

impl NodePhase {
    /// The coarse label `/health` publishes.
    pub fn label(&self) -> &'static str {
        match self {
            NodePhase::Starting => "starting",
            NodePhase::Syncing => "syncing",
            NodePhase::Running => "running",
            NodePhase::Blocked(_) => "blocked",
        }
    }

    /// **Deploy admission is about the pool, not readiness.** `Syncing` and `Blocked` are the two
    /// phases in which telling a client "accepted" would be a promise the node cannot keep: it is
    /// restoring a chain it is not serving, or it has refused to serve. `Starting` is launch still
    /// deciding — the deploy can be pooled and is included when the node reaches `Running`, or
    /// after a sync; refusing it here would turn a millisecond-wide boot window into a client-visible
    /// 503 for a deploy the node was seconds from accepting (the `deploy_is_processed_into_a_block`
    /// integration test hit exactly that window). `/health`'s `ready` is still false in `Starting`;
    /// admission and readiness answer different questions.
    pub fn admits_work(&self) -> bool {
        !matches!(self, NodePhase::Syncing | NodePhase::Blocked(_))
    }
}

static PHASE: Mutex<NodePhase> = Mutex::new(NodePhase::Starting);

pub fn phase() -> NodePhase {
    PHASE.lock().unpoison().clone()
}

/// **A process that has reached the running engine never goes back down.** The cell is one per
/// process (the production case is one node, one process), but a test harness runs several nodes in
/// one process and a multi-shard node launches each shard separately: a second launch setting
/// `Starting`/`Syncing` must not take readiness away from a node that is already serving. The
/// conservative direction is chosen deliberately — a second node's sync is under-reported in that
/// process rather than halting the first node's production. Per-shard/per-node phase is owed; see
/// the module docs.
pub fn set_phase(next: NodePhase) {
    let mut guard = PHASE.lock().unpoison();
    if matches!(*guard, NodePhase::Running) && !matches!(next, NodePhase::Running) {
        return;
    }
    *guard = next;
}

pub fn is_running() -> bool {
    matches!(*PHASE.lock().unpoison(), NodePhase::Running)
}

/// **Block the caller until launch has reached the running engine.** A loop rather than a watch
/// channel because the readers are long-lived tasks (the disposition loops) and the writer is
/// launch; the poll interval is far below any human-visible delay and costs nothing.
pub async fn wait_until_running() {
    while !is_running() {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// How long a propose may wait for launch to decide before it is answered "not ready".
///
/// The proposer stream starts **beside** `node_launch`, not after it (`setup_shard_runtime` spawns
/// launch and then constructs the stream), so a propose that arrives while a genesis master is still
/// in its boot repair sees `Starting`. Refusing immediately turned a healthy boot into a failure for
/// the client; waiting for ever would turn a hung launch into a hung propose. Two minutes is far
/// above any observed launch decision and far below a client's patience.
pub const PHASE_DECISION_WAIT: Duration = Duration::from_secs(120);

/// Wait for launch to leave `Starting` (or for the bounded window to close) and return the phase a
/// propose should be judged against.
pub async fn wait_until_decided() -> NodePhase {
    let deadline = tokio::time::Instant::now() + PHASE_DECISION_WAIT;
    loop {
        let current = phase();
        if !matches!(current, NodePhase::Starting) {
            return current;
        }
        if tokio::time::Instant::now() >= deadline {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
