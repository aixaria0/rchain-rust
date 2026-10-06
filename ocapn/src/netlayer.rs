//! The netlayer abstraction: a bidirectional FIFO between two peers.
//!
//! `draft-specifications/Netlayers.md` and the implementation guide present two functions —
//! `new_outgoing_connection(ocapn_locator)` and `accept_incoming_connection()` — over a channel
//! that is "a bidirectional FIFO". CapTP stays "agnostic to latency, liveness, and privacy
//! characteristics of each underlying protocol": those are the netlayer's business, and the
//! protocol above sees only a queue of messages.
//!
//! A message on the channel is one CapTP operation, carried as one Syrup value. The only netlayer
//! implemented here is [`crate::tcp_testing_only`], the conformance suite's transport; a
//! production netlayer (Tor, libp2p, IBC) would implement the same two functions and nothing above
//! would change.

use std::io;

use async_trait::async_trait;

use crate::locator::PeerLocator;

/// One open channel to a peer.
#[async_trait]
pub trait NetConn: Send {
    /// Write one CapTP message — a single Syrup value's bytes.
    async fn send(&mut self, message: &[u8]) -> io::Result<()>;

    /// **The peer's own address**, when the transport knows it (Law 62, AUDIT C225).
    ///
    /// The dial policy needs it to tell a *remote* peer from a local one: a peer on another host must
    /// not be able to make this node reach the node's own loopback services. A transport that cannot
    /// say is unchanged — the default is `None`, which the policy reads as "cannot be judged".
    fn peer_address(&self) -> Option<std::net::SocketAddr> {
        None
    }

    /// Read one CapTP message. `Ok(None)` is a clean end of stream *at a message boundary*; a
    /// stream that ends in the middle of a message is an error, never a silently dropped message.
    async fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;
}

/// How a peer is dialled, and how it accepts dials — the two functions the netlayer standard
/// fixes.
#[async_trait]
pub trait Netlayer: Send + Sync {
    /// `new_outgoing_connection(ocapn_locator)` — dial the peer the locator names.
    async fn new_outgoing_connection(&self, locator: &PeerLocator) -> io::Result<Box<dyn NetConn>>;

    /// The same dial, told **which peer asked for it** (Law 62): `origin` is the address of the
    /// session the request arrived on, when the transport knows it. A dial a *remote* peer asked for
    /// is refused when the target is one of this node's own local addresses, so a peer cannot use this
    /// node to reach what the peer itself cannot.
    ///
    /// **Provided, and defaulting to the origin-free dial.** The netlayer standard fixes two
    /// functions, and a transport that cannot report an origin must stay a netlayer — so this is a
    /// refinement [`crate::dial_policy::PolicyNetlayer`] opts into, not a third function every
    /// implementation has to write.
    async fn new_outgoing_connection_from(
        &self,
        locator: &PeerLocator,
        _origin: Option<std::net::SocketAddr>,
    ) -> io::Result<Box<dyn NetConn>> {
        self.new_outgoing_connection(locator).await
    }

    /// `accept_incoming_connection()` — await the next peer that dials us.
    async fn accept_incoming_connection(&self) -> io::Result<Box<dyn NetConn>>;
}
