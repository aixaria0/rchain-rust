//! Block retriever (port of `blocks/BlockRetriever.scala`).
//!
//! Makes sure a block is received once casper requests it. A block stays in scope until it is added
//! to the casper buffer (acknowledged via [`BlockRetriever::ack_received`]).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

use rchain_comm::peer_node::PeerNode;
use rchain_models::block_hash::BlockHash;
use rchain_shared::log::{Log, LogSource};
use rchain_shared::time::current_millis;

use crate::protocol::comm_util::CommUtil;

/// Maximum number of distinct hashes the retriever tracks at once (H5): bounds the `requested` map
/// against a peer flooding `BlockHash`/`HasBlock` messages for bogus hashes.
const MAX_REQUESTED_BLOCKS: usize = 10_000;
/// Maximum waiting-list length per requested hash (H5).
const MAX_WAITING_LIST_PER_HASH: usize = 32;
/// At most two distinct sources may receive an eager request for a hash.
/// A paused validator must not delay the first responsive alternative for 240 seconds.
const MAX_EAGER_BLOCK_SOURCES: usize = 2;
/// Independent periodic retries do not wait in a serial chain on an unreachable peer.
const MAX_PARALLEL_BLOCK_RETRIES: usize = 16;

/// Reason a hash was admitted (port of `AdmitHashReason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmitHashReason {
    HasBlockMessageReceived,
    HashBroadcastRecieved,
    MissingDependencyRequested,
    BlockReceived,
}

/// Status of an admitted hash (port of `AdmitHashStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmitHashStatus {
    NewSourcePeerAddedToRequest,
    NewRequestAdded,
    /// The `requested` map is at capacity; the new hash was not admitted.
    CapacityReached,
    Ignore,
}

/// Result of admitting a hash (port of `AdmitHashResult`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmitHashResult {
    pub status: AdmitHashStatus,
    pub broadcast_request: bool,
    pub request_block: bool,
}

/// Per-hash request state (port of `RequestState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestState {
    /// Last time the block was requested.
    pub timestamp: i64,
    /// Peers that were queried for this block.
    pub peers: Vec<PeerNode>,
    /// Peers that reportedly have the block and are yet to be queried.
    pub waiting_list: Vec<PeerNode>,
}

impl RequestState {
    fn new(now: i64, source_peer: Option<PeerNode>) -> Self {
        RequestState {
            timestamp: now,
            // Eager sources are *already contacted*, not pending retry candidates.
            peers: source_peer.into_iter().collect(),
            waiting_list: Vec::new(),
        }
    }
}

/// Add a new request if the hash is unknown (port of `addNewRequest`).
fn add_new_request(
    state: &BTreeMap<BlockHash, RequestState>,
    hash: &BlockHash,
    now: i64,
    source_peer: Option<PeerNode>,
) -> BTreeMap<BlockHash, RequestState> {
    if state.contains_key(hash) {
        state.clone()
    } else {
        let mut new_state = state.clone();
        new_state.insert(*hash, RequestState::new(now, source_peer));
        new_state
    }
}

/// Register a previously unseen source. A second eager source protects delivery
/// against a deliberately paused initial peer; further sources remain queued for
/// the unchanged 240-second retry policy. Neither list grows without a bound.
fn add_source_peer_to_request(
    state: &BTreeMap<BlockHash, RequestState>,
    hash: &BlockHash,
    peer: &PeerNode,
) -> (BTreeMap<BlockHash, RequestState>, bool, bool) {
    let Some(request) = state.get(hash) else {
        return (state.clone(), false, false);
    };
    if request.peers.contains(peer) || request.waiting_list.contains(peer) {
        return (state.clone(), false, false);
    }
    let eager = request.peers.len() < MAX_EAGER_BLOCK_SOURCES;
    if !eager && request.waiting_list.len() >= MAX_WAITING_LIST_PER_HASH {
        return (state.clone(), false, false);
    }
    let mut updated = request.clone();
    if eager {
        updated.peers.push(peer.clone());
    } else {
        updated.waiting_list.push(peer.clone());
    }
    let mut new_state = state.clone();
    new_state.insert(*hash, updated);
    (new_state, true, eager)
}

/// The pure state transition for `admit_hash` (port of the `RequestedBlocks.modify` block).
pub fn admit_hash_state(
    state: &BTreeMap<BlockHash, RequestState>,
    hash: &BlockHash,
    now: i64,
    peer: Option<&PeerNode>,
) -> (BTreeMap<BlockHash, RequestState>, AdmitHashResult) {
    let unknown_hash = !state.contains_key(hash);
    if unknown_hash {
        if state.len() >= MAX_REQUESTED_BLOCKS {
            return (
                state.clone(),
                AdmitHashResult {
                    status: AdmitHashStatus::CapacityReached,
                    broadcast_request: false,
                    request_block: false,
                },
            );
        }
        let new_state = add_new_request(state, hash, now, peer.cloned());
        let result = AdmitHashResult {
            status: AdmitHashStatus::NewRequestAdded,
            broadcast_request: peer.is_none(),
            request_block: peer.is_some(),
        };
        (new_state, result)
    } else {
        match peer {
            Some(peer) => {
                let (new_state, added, eager) = add_source_peer_to_request(state, hash, peer);
                (
                    new_state,
                    AdmitHashResult {
                        status: if added {
                            AdmitHashStatus::NewSourcePeerAddedToRequest
                        } else {
                            AdmitHashStatus::Ignore
                        },
                        broadcast_request: false,
                        request_block: eager,
                    },
                )
            }
            None => (
                state.clone(),
                AdmitHashResult {
                    status: AdmitHashStatus::Ignore,
                    broadcast_request: false,
                    request_block: false,
                },
            ),
        }
    }
}

/// Block retriever (port of `BlockRetriever[F]`).
pub struct BlockRetriever {
    requested: tokio::sync::Mutex<BTreeMap<BlockHash, RequestState>>,
    comm_util: Arc<CommUtil>,
    log: Arc<dyn Log>,
    log_source: LogSource,
}

impl BlockRetriever {
    pub fn new(comm_util: Arc<CommUtil>, log: Arc<dyn Log>) -> Self {
        BlockRetriever {
            requested: tokio::sync::Mutex::new(BTreeMap::new()),
            comm_util,
            log,
            log_source: LogSource::new("casper.blocks.BlockRetriever"),
        }
    }

    /// Make the retriever process an incoming hash (port of `admitHash`).
    pub async fn admit_hash(
        &self,
        hash: &BlockHash,
        peer: Option<&PeerNode>,
        reason: AdmitHashReason,
    ) -> AdmitHashResult {
        let now = current_millis();
        let result = {
            let mut state = self.requested.lock().await;
            let (new_state, result) = admit_hash_state(&state, hash, now, peer);
            *state = new_state;
            result
        };

        match result.status {
            AdmitHashStatus::NewSourcePeerAddedToRequest => {
                self.log.debug(
                    self.log_source,
                    &format!(
                        "Adding {} to waiting list of {} request. Reason: {:?}",
                        peer.map(|p| p.endpoint.host.as_str()).unwrap_or(""),
                        hash.to_hex(),
                        reason
                    ),
                );
            }
            AdmitHashStatus::NewRequestAdded => {
                self.log.info(
                    self.log_source,
                    &format!(
                        "Adding {} hash to RequestedBlocks because of {:?}.",
                        hash.to_hex(),
                        reason
                    ),
                );
            }
            AdmitHashStatus::CapacityReached => {
                self.log.warn(
                    self.log_source,
                    &format!(
                        "Dropping {} hash: block-retriever request map is at capacity ({}).",
                        hash.to_hex(),
                        MAX_REQUESTED_BLOCKS
                    ),
                );
            }
            AdmitHashStatus::Ignore => {}
        }

        if result.broadcast_request {
            self.comm_util.broadcast_has_block_request(hash).await;
        }
        if result.request_block {
            if let Some(peer) = peer {
                self.comm_util.request_for_block(peer, hash).await;
            }
        }
        result
    }

    /// Acknowledge that a block was received (port of `ackReceived`).
    pub async fn ack_received(&self, hash: &BlockHash) {
        let mut state = self.requested.lock().await;
        let old = state.clone();
        state.remove(hash);
        let is_received = *state != old;
        if is_received {
            self.log.info(
                self.log_source,
                &format!("Block {} marked as received.", hash.to_hex()),
            );
        }
    }

    /// Try to re-request all expired hashes. The 240-second retry eligibility
    /// remains unchanged, but one unreachable peer no longer serially stalls
    /// the entire retriever maintenance task.
    pub async fn request_all(self: &Arc<Self>, age_threshold: Duration) {
        let keys: Vec<BlockHash> = { self.requested.lock().await.keys().copied().collect() };
        if !keys.is_empty() {
            self.log.debug(
                self.log_source,
                &format!(
                    "Running BlockRetriever maintenance ({} items in scope).",
                    keys.len()
                ),
            );
        }
        let mut pending = JoinSet::new();
        for hash in keys {
            while let Some(result) = pending.try_join_next() {
                if let Err(error) = result {
                    self.log.error(
                        self.log_source,
                        &format!("Block retriever retry task failed: {error}"),
                    );
                }
            }
            if pending.len() >= MAX_PARALLEL_BLOCK_RETRIES {
                if let Some(Err(error)) = pending.join_next().await {
                    self.log.error(
                        self.log_source,
                        &format!("Block retriever retry task failed: {error}"),
                    );
                }
            }
            let retriever = self.clone();
            pending.spawn(async move {
                retriever.try_rerequest(hash, age_threshold).await;
            });
        }
        while let Some(result) = pending.join_next().await {
            if let Err(error) = result {
                self.log.error(
                    self.log_source,
                    &format!("Block retriever retry task failed: {error}"),
                );
            }
        }
    }

    /// Choose and record the next source *before* awaiting transport.
    /// In particular a response arriving during the timeout must not be
    /// overwritten by a stale snapshot, and ack_received must not be undone.
    async fn try_rerequest(&self, hash: BlockHash, age_threshold: Duration) {
        let next = {
            let mut state = self.requested.lock().await;
            prepare_expired_retry(&mut state, &hash, current_millis(), age_threshold)
        };
        match next {
            Some(RetryAction::Peer(peer)) => {
                self.log.debug(
                    self.log_source,
                    &format!(
                        "Trying {} to query for {} block.",
                        peer.endpoint.host,
                        hash.to_hex()
                    ),
                );
                self.comm_util.request_for_block(&peer, &hash).await;
            }
            Some(RetryAction::Broadcast) => {
                self.log.warn(
                    self.log_source,
                    &format!(
                        "Could not retrieve requested block {}. Asking peers again.",
                        hash.to_hex()
                    ),
                );
                self.comm_util.broadcast_has_block_request(&hash).await;
            }
            None => {}
        }
    }
}

/// A request is either atomically advanced to a fresh source, reannounced, or
/// skipped if already acknowledged or not yet expired.
#[derive(Debug, PartialEq, Eq)]
enum RetryAction {
    Peer(PeerNode),
    Broadcast,
}

fn prepare_expired_retry(
    state: &mut BTreeMap<BlockHash, RequestState>,
    hash: &BlockHash,
    now: i64,
    age_threshold: Duration,
) -> Option<RetryAction> {
    let request = state.get_mut(hash)?;
    let age_ms = i64::try_from(age_threshold.as_millis()).unwrap_or(i64::MAX);
    if now.saturating_sub(request.timestamp) <= age_ms {
        return None;
    }
    if !request.waiting_list.is_empty() {
        let next_peer = request.waiting_list.remove(0);
        if !request.peers.contains(&next_peer) {
            request.peers.push(next_peer.clone());
        }
        request.timestamp = now;
        Some(RetryAction::Peer(next_peer))
    } else {
        state.remove(hash);
        Some(RetryAction::Broadcast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rchain_comm::errors::CommErr;
    use rchain_comm::peer_node::NodeIdentifier;
    use rchain_comm::rp::rp_conf::{ClearConnectionsConf, RPConf};
    use rchain_comm::transport::transport_layer::TransportLayer;
    use rchain_models::comm::protocol::Protocol;
    use rchain_shared::log::NopLog;

    use crate::protocol::comm_util::{CommUtil, ConnectionsCell};

    fn hash(byte: u8) -> BlockHash {
        BlockHash::new([byte; 32])
    }

    fn peer(name: &str, port: u16) -> PeerNode {
        PeerNode::from(
            NodeIdentifier::new(name.as_bytes().to_vec()),
            "host".to_string(),
            rchain_shared::refined::Port::new(port),
            rchain_shared::refined::Port::new(port),
        )
    }

    fn set(items: &[u8]) -> BTreeMap<BlockHash, RequestState> {
        items
            .iter()
            .map(|b| (hash(*b), RequestState::new(0, None)))
            .collect()
    }

    #[test]
    fn admit_hash_state_adds_unknown_hash_and_broadcasts_without_peer() {
        let state = set(&[]);
        let (new_state, result) = admit_hash_state(&state, &hash(1), 0, None);
        assert!(new_state.contains_key(&hash(1)));
        assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
        assert!(result.broadcast_request);
        assert!(!result.request_block);
    }

    #[test]
    fn admit_hash_state_adds_unknown_hash_and_requests_with_peer() {
        let p = peer("peer", 40400);
        let state = set(&[]);
        let (new_state, result) = admit_hash_state(&state, &hash(1), 0, Some(&p));
        assert!(new_state.contains_key(&hash(1)));
        assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
        assert!(!result.broadcast_request);
        assert!(result.request_block);
    }

    #[test]
    fn admit_hash_state_ignores_known_hash_without_peer() {
        let state = set(&[1]);
        let (_, result) = admit_hash_state(&state, &hash(1), 0, None);
        assert_eq!(result.status, AdmitHashStatus::Ignore);
        assert!(!result.broadcast_request);
        assert!(!result.request_block);
    }

    #[test]
    fn admit_hash_state_requests_from_first_peer_when_waiting_list_empty() {
        let p = peer("peer", 40400);
        let state = set(&[1]); // known hash, empty waiting list
        let (_, result) = admit_hash_state(&state, &hash(1), 0, Some(&p));
        assert_eq!(result.status, AdmitHashStatus::NewSourcePeerAddedToRequest);
        assert!(result.request_block);
    }

    #[test]
    fn admit_hash_state_ignores_peer_already_in_waiting_list() {
        let p = peer("peer", 40400);
        let mut state = set(&[1]);
        state
            .get_mut(&hash(1))
            .unwrap()
            .waiting_list
            .push(p.clone());
        let (_, result) = admit_hash_state(&state, &hash(1), 0, Some(&p));
        assert_eq!(result.status, AdmitHashStatus::Ignore);
    }

    #[test]
    fn second_distinct_source_is_eager_but_third_waits_for_retry() {
        let a = peer("first", 40400);
        let b = peer("second", 40401);
        let c = peer("third", 40402);
        let (state, first) = admit_hash_state(&set(&[]), &hash(1), 10, Some(&a));
        assert!(first.request_block);
        assert_eq!(state[&hash(1)].peers, vec![a.clone()]);
        let (state, second) = admit_hash_state(&state, &hash(1), 11, Some(&b));
        assert!(second.request_block);
        let (state, third) = admit_hash_state(&state, &hash(1), 12, Some(&c));
        assert!(!third.request_block);
        assert_eq!(state[&hash(1)].peers, vec![a.clone(), b]);
        assert_eq!(state[&hash(1)].waiting_list, vec![c]);
        let (_, duplicate) = admit_hash_state(&state, &hash(1), 13, Some(&a));
        assert_eq!(duplicate.status, AdmitHashStatus::Ignore);
        assert!(!duplicate.request_block);
    }

    #[test]
    fn expired_retry_commits_transition_once_and_ack_cannot_resurrect_it() {
        let a = peer("first", 40400);
        let b = peer("second", 40401);
        let c = peer("third", 40402);
        let (state, _) = admit_hash_state(&set(&[]), &hash(1), 10, Some(&a));
        let (state, _) = admit_hash_state(&state, &hash(1), 11, Some(&b));
        let (mut state, _) = admit_hash_state(&state, &hash(1), 12, Some(&c));
        let action = prepare_expired_retry(&mut state, &hash(1), 250, Duration::from_millis(100));
        assert_eq!(action, Some(RetryAction::Peer(c)));
        assert_eq!(state[&hash(1)].timestamp, 250);
        assert!(state[&hash(1)].waiting_list.is_empty());
        assert_eq!(
            prepare_expired_retry(&mut state, &hash(1), 260, Duration::from_millis(100)),
            None
        );
        state.remove(&hash(1)); // ack_received while the transport request is still in flight
        assert_eq!(
            prepare_expired_retry(&mut state, &hash(1), 500, Duration::from_millis(100)),
            None
        );
        assert!(!state.contains_key(&hash(1)));
    }

    #[test]
    fn bounded_waiting_sources_do_not_grow_on_duplicates() {
        let a = peer("first", 40400);
        let b = peer("second", 40401);
        let (state, _) = admit_hash_state(&set(&[]), &hash(2), 0, Some(&a));
        let (mut state, _) = admit_hash_state(&state, &hash(2), 0, Some(&b));
        for n in 0..(MAX_WAITING_LIST_PER_HASH + 8) {
            let p = peer(&format!("extra-{n}"), 40500 + n as u16);
            (state, _) = admit_hash_state(&state, &hash(2), 0, Some(&p));
        }
        assert_eq!(
            state[&hash(2)].waiting_list.len(),
            MAX_WAITING_LIST_PER_HASH
        );
        assert_eq!(state[&hash(2)].peers.len(), MAX_EAGER_BLOCK_SOURCES);
        let (_, duplicate) = admit_hash_state(&state, &hash(2), 0, Some(&a));
        assert_eq!(duplicate.status, AdmitHashStatus::Ignore);
    }

    // --- effectful integration (mock transport) -------------------------------------------

    #[derive(Default)]
    struct MockTransport {
        sends: std::sync::Mutex<Vec<(PeerNode, Protocol)>>,
        broadcasts: std::sync::Mutex<Vec<(Vec<PeerNode>, Protocol)>>,
    }

    #[async_trait]
    impl TransportLayer for MockTransport {
        async fn send(&self, peer: &PeerNode, msg: Protocol) -> CommErr<()> {
            self.sends.lock().unwrap().push((peer.clone(), msg));
            Ok(())
        }
        async fn broadcast(&self, peers: &[PeerNode], msg: Protocol) -> Vec<CommErr<()>> {
            self.broadcasts.lock().unwrap().push((peers.to_vec(), msg));
            peers.iter().map(|_| Ok(())).collect()
        }
        async fn stream(&self, _peers: &[PeerNode], _blob: rchain_comm::transport::chunker::Blob) {}
    }

    fn conf(local: &PeerNode) -> RPConf {
        RPConf {
            local: local.clone(),
            network_id: "testnet".to_string(),
            bootstrap: None,
            default_timeout: Duration::from_secs(10),
            max_num_of_connections: 10,
            clear_connections: ClearConnectionsConf {
                num_of_connections_pinged: 10,
            },
        }
    }

    fn retriever(transport: Arc<MockTransport>, local: &PeerNode) -> BlockRetriever {
        let connections: ConnectionsCell = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let comm_util = Arc::new(CommUtil::new(
            transport,
            conf(local),
            connections,
            Arc::new(NopLog),
        ));
        BlockRetriever::new(comm_util, Arc::new(NopLog))
    }

    /// A paused first source must not monopolize a hash that a second peer
    /// advertises. The transport intentionally never completes until released.
    #[derive(Default)]
    struct PausedFirstTransport {
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
        attempts: std::sync::Mutex<Vec<PeerNode>>,
    }

    #[async_trait]
    impl TransportLayer for PausedFirstTransport {
        async fn send(&self, peer: &PeerNode, _msg: Protocol) -> CommErr<()> {
            self.attempts.lock().unwrap().push(peer.clone());
            if u16::from(peer.endpoint.tcp_port) == 40401 {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(())
        }
        async fn broadcast(&self, peers: &[PeerNode], _msg: Protocol) -> Vec<CommErr<()>> {
            peers.iter().map(|_| Ok(())).collect()
        }
        async fn stream(&self, _peers: &[PeerNode], _blob: rchain_comm::transport::chunker::Blob) {}
    }

    #[tokio::test]
    async fn second_source_request_completes_while_first_transport_is_paused() {
        let transport = Arc::new(PausedFirstTransport::default());
        let local = peer("local", 40400);
        let initial = peer("unresponsive", 40401);
        let backup = peer("responsive", 40402);
        let connections: ConnectionsCell = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let comm = Arc::new(CommUtil::new(
            transport.clone(),
            conf(&local),
            connections,
            Arc::new(NopLog),
        ));
        let retriever = Arc::new(BlockRetriever::new(comm, Arc::new(NopLog)));
        let first_retriever = retriever.clone();
        let first_peer = initial.clone();
        let first = tokio::spawn(async move {
            first_retriever
                .admit_hash(
                    &hash(7),
                    Some(&first_peer),
                    AdmitHashReason::HasBlockMessageReceived,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), transport.started.notified())
            .await
            .expect("first transport must be paused");
        let second = tokio::time::timeout(
            Duration::from_secs(1),
            retriever.admit_hash(
                &hash(7),
                Some(&backup),
                AdmitHashReason::HasBlockMessageReceived,
            ),
        )
        .await
        .expect("second peer must not wait on the first peer's transport");
        assert_eq!(second.status, AdmitHashStatus::NewSourcePeerAddedToRequest);
        assert!(second.request_block, "backup must be queried eagerly");
        assert_eq!(
            transport.attempts.lock().unwrap().as_slice(),
            &[initial, backup]
        );
        retriever.ack_received(&hash(7)).await;
        transport.release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .expect("first request must complete after release")
            .unwrap();
        assert!(!retriever.requested.lock().await.contains_key(&hash(7)));
    }

    #[tokio::test]
    async fn admit_hash_with_peer_sends_block_request() {
        let transport = Arc::new(MockTransport::default());
        let local = peer("src", 40400);
        let remote = peer("peer", 40400);
        let retriever = retriever(transport.clone(), &local);

        let result = retriever
            .admit_hash(
                &hash(1),
                Some(&remote),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await;
        assert!(result.request_block);
        assert!(!result.broadcast_request);

        let sends = transport.sends.lock().unwrap();
        assert_eq!(sends.len(), 1);
        let (recipient, msg) = &sends[0];
        assert_eq!(recipient, &remote);
        let packet = rchain_comm::rp::protocol_helper::to_packet(msg).unwrap();
        assert_eq!(packet.type_id, "BlockRequest");
    }

    #[tokio::test]
    async fn admit_hash_without_peer_broadcasts_has_block_request() {
        let transport = Arc::new(MockTransport::default());
        let local = peer("src", 40400);
        let retriever = retriever(transport.clone(), &local);

        let result = retriever
            .admit_hash(&hash(1), None, AdmitHashReason::HashBroadcastRecieved)
            .await;
        assert!(result.broadcast_request);
        assert!(!result.request_block);

        let broadcasts = transport.broadcasts.lock().unwrap();
        assert_eq!(broadcasts.len(), 1);
        let (_, msg) = &broadcasts[0];
        let packet = rchain_comm::rp::protocol_helper::to_packet(msg).unwrap();
        assert_eq!(packet.type_id, "HasBlockRequest");
    }
}
