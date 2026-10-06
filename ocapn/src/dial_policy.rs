//! Which addresses this peer will dial on a peer's word.
//!
//! **The peer chooses the address.** A sturdyref carries a peer locator and a handoff give carries an
//! `exporter-location`; both arrive over the wire (`enliven.rs`, `fixtures.rs`), and both end at
//! `Netlayer::new_outgoing_connection`. A transport with no policy is therefore an SSRF primitive: a
//! peer can make the node connect to anything the node can reach — the classic target being the cloud
//! metadata endpoint at `169.254.169.254`, and the useful one a scan of the operator's LAN
//! (the HAZOP's row B4, measured: the attacker's server received our `op:start-session`).
//!
//! **What this closes, and what it does not.** Link-local and the unspecified address are never a
//! legitimate CapTP peer, so they are refused with no configuration. Loopback and private ranges *are*
//! legitimate for a local demo or a devnet — the conformance suite and the ERTP transcript both run
//! over loopback — so they are permitted by default and can be denied (`deny_local`) by an operator
//! who is serving a peer that is not on their own host.
//!
//! **A name is resolved before it is judged**, because checking only the spelling is the classic way
//! to evade a policy like this; a name that does not resolve is refused, since the dial would fail
//! anyway.
//!
//! **A transport with no `host` hint is not judged at all**, and neither is a session whose origin the
//! transport cannot report. `unix` is both: its locator carries a path rather than a host, and a
//! Unix-socket peer has no `SocketAddr` for `NetConn::peer_address` to return. That is the right
//! answer rather than a hole — such a peer is local by construction, and what admitted it is the
//! **socket's filesystem permission**, which is the transport's authentication. The rule this file
//! enforces is about peers that arrive over a network; a peer the filesystem let in is not one.
//!
//! **And the peer's own origin decides what *it* may reach** (Law 62, AUDIT C225). A dial a *remote*
//! peer asked for is refused when the target is one of this node's own local addresses: the peer would
//! be using this node to reach a service it cannot reach itself, which is what an SSRF is. A peer that
//! *is* local is dialling its own neighbourhood, which is what a demo and the conformance suite do —
//! so the rule keys on the origin rather than on the target alone, and a transport that cannot report
//! an origin keeps the behaviour it had.

use crate::locator::PeerLocator;
use crate::netlayer::{NetConn, Netlayer};

/// The policy a peer's dial is measured against.
#[derive(Clone, Debug)]
pub struct DialPolicy {
    /// Refuse loopback and private ranges too. Off by default: a loopback peer is how this crate is
    /// tested and demonstrated.
    pub deny_local: bool,
    /// Addresses (or host names) that are permitted even when they would otherwise be refused.
    pub allow: Vec<String>,
}

impl Default for DialPolicy {
    fn default() -> Self {
        DialPolicy {
            deny_local: false,
            allow: Vec::new(),
        }
    }
}

impl DialPolicy {
    /// A policy that refuses nothing. For the fixture peer, which must dial whatever the suite names.
    pub fn permit_all() -> Self {
        DialPolicy {
            deny_local: false,
            allow: Vec::new(),
        }
    }

    /// The host a locator names, if it names one — from the `host` hint, **or from the authority of a
    /// `url` hint**.
    ///
    /// **A `url` hint is a host by another name, and this policy was blind to it.** The `websocket`
    /// transport carries its whole address in `url` (`ws://host:port`), so a policy that keyed on
    /// `host` alone returned `Ok` for every websocket target: the link-local and metadata refusals and
    /// Law 62's origin rule were **skipped entirely**, and a sturdyref naming
    /// `ws://169.254.169.254/` was dialled. The HAZOP's red team measured it — with
    /// `ocapn-deny-local-dial = true` the node refused a loopback target over `tcp-testing-only` and
    /// *connected* to one over `websocket` (row E10).
    ///
    /// A transport whose locator carries neither — `unix`, whose `path` is not an address — still
    /// yields `None`, which is the "cannot be judged" answer the module doc describes and the right
    /// one for a peer the filesystem admitted.
    fn host_of(locator: &PeerLocator) -> Option<String> {
        if let Some(host) = locator.hints.get("host") {
            return Some(host.clone());
        }
        let url = locator.hints.get("url")?;
        let authority = url
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(url.as_str())
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("");
        let host = match authority.strip_prefix('[') {
            // A bracketed IPv6 literal: `[fe80::1]:9045`.
            Some(after) => after.split(']').next().unwrap_or(after).to_string(),
            // `host:port`, or a bare host.
            None => authority
                .rsplit_once(':')
                .map(|(host, _)| host)
                .unwrap_or(authority)
                .to_string(),
        };
        (!host.is_empty()).then_some(host)
    }

    /// Whether a dial to `locator` is permitted, or why it is not.
    pub fn permits(&self, locator: &PeerLocator) -> Result<(), String> {
        let Some(host) = Self::host_of(locator) else {
            // A transport whose hints name no host — `unix`'s `path`, say — cannot be checked, and
            // refusing everything that lacks one would break every netlayer that reaches its peer
            // another way.
            return Ok(());
        };
        if self.allow.iter().any(|a| *a == host) {
            return Ok(());
        }
        let address = match host.parse::<std::net::IpAddr>() {
            Ok(address) => address,
            // **A name is resolved before it is judged.** Checking only the spelling let a name that
            // resolves into a denied range through — the classic defence-evasion against exactly this
            // kind of policy. A name that does not resolve is *refused*: the dial would fail anyway,
            // and refusing here keeps the decision in one place. Each resolved address is checked, and
            // any denied one refuses the whole name (a name resolving to both a public and a link-local
            // address is not a name this peer should be following).
            Err(_) => {
                let Ok(addresses) = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0))
                else {
                    return Err(format!(
                        "this peer does not dial {host}: it does not resolve, so the address is the \
                         peer's and cannot be judged"
                    ));
                };
                for candidate in addresses {
                    self.permits_address(candidate.ip())?;
                }
                return Ok(());
            }
        };
        self.permits_address(address)
    }

    /// Whether a dial to `locator` is permitted **for the peer at `origin`** — the socket address of
    /// the session the dial was asked for on, when the transport knows it (Law 62, AUDIT C225).
    ///
    /// The first half is [`DialPolicy::permits`], unchanged. The second is the one this exists for: a
    /// peer that is **not** on this host may not make this node reach this node's own local
    /// addresses. A peer that *is* local may: loopback is how this crate is tested and demonstrated,
    /// and a loopback peer reaching loopback is not an escalation.
    ///
    /// `None` means the transport cannot say where the peer is, and the dial is judged as before —
    /// refusing on an unknowable origin would break every netlayer that cannot report one.
    pub fn permits_from(
        &self,
        locator: &PeerLocator,
        origin: Option<std::net::SocketAddr>,
    ) -> Result<(), String> {
        self.permits(locator)?;
        let Some(origin) = origin else {
            return Ok(());
        };
        let Some(host) = Self::host_of(locator) else {
            return Ok(());
        };
        if self.allow.iter().any(|a| *a == host) {
            return Ok(());
        }
        if origin.ip().is_loopback() {
            return Ok(());
        }
        let who = format!("a peer at {} (not on this host)", origin);
        match host.parse::<std::net::IpAddr>() {
            Ok(address) => self.not_a_local_target(address, &who),
            Err(_) => {
                let Ok(addresses) = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0))
                else {
                    // A name that does not resolve was refused by `permits` above.
                    return Ok(());
                };
                for candidate in addresses {
                    self.not_a_local_target(candidate.ip(), &who)?;
                }
                Ok(())
            }
        }
    }

    /// **The origin rule**: a remote peer may not aim this node at one of its own local addresses.
    fn not_a_local_target(&self, address: std::net::IpAddr, who: &str) -> Result<(), String> {
        let local = match address {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
            }
        };
        if local {
            return Err(format!(
                "{who} cannot make this node dial {address}: it is one of this node's own local \
                 addresses, so the peer is asking this node to reach a service the peer itself cannot"
            ));
        }
        Ok(())
    }

    /// The address half of [`DialPolicy::permits`], which a resolved name is checked with too.
    fn permits_address(&self, address: std::net::IpAddr) -> Result<(), String> {
        let what = match address {
            std::net::IpAddr::V4(v4) => {
                if v4.is_link_local() {
                    Some("link-local (169.254.0.0/16 — the cloud metadata range)")
                } else if v4.is_unspecified() {
                    Some("the unspecified address")
                } else if self.deny_local && (v4.is_loopback() || v4.is_private()) {
                    Some("loopback or a private range")
                } else {
                    None
                }
            }
            std::net::IpAddr::V6(v6) => {
                let link_local = (v6.segments()[0] & 0xffc0) == 0xfe80;
                if link_local {
                    Some("link-local (fe80::/10)")
                } else if v6.is_unspecified() {
                    Some("the unspecified address")
                } else if self.deny_local
                    && (v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00)
                {
                    Some("loopback or a unique-local range")
                } else {
                    None
                }
            }
        };
        match what {
            Some(what) => Err(format!(
                "this peer does not dial {address}: {what}. A sturdyref or a handoff give named it, \
                 and neither is authenticated, so the address is the peer's choice"
            )),
            None => Ok(()),
        }
    }
}

/// A netlayer that applies a [`DialPolicy`] before delegating.
pub struct PolicyNetlayer<N> {
    inner: N,
    policy: DialPolicy,
}

impl<N> PolicyNetlayer<N> {
    pub fn new(inner: N, policy: DialPolicy) -> Self {
        PolicyNetlayer { inner, policy }
    }
}

#[async_trait::async_trait]
impl<N: Netlayer> Netlayer for PolicyNetlayer<N> {
    async fn new_outgoing_connection(
        &self,
        locator: &PeerLocator,
    ) -> std::io::Result<Box<dyn NetConn>> {
        self.new_outgoing_connection_from(locator, None).await
    }

    /// The dial, judged against **the peer that asked for it** (Law 62).
    async fn new_outgoing_connection_from(
        &self,
        locator: &PeerLocator,
        origin: Option<std::net::SocketAddr>,
    ) -> std::io::Result<Box<dyn NetConn>> {
        // **Refused before the connect, not after.** The point is to not make the connection: a
        // policy that dials and hangs up has already told the peer whether something is listening.
        self.policy
            .permits_from(locator, origin)
            .map_err(|reason| std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason))?;
        self.inner.new_outgoing_connection(locator).await
    }

    async fn accept_incoming_connection(&self) -> std::io::Result<Box<dyn NetConn>> {
        self.inner.accept_incoming_connection().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn locator(host: &str) -> PeerLocator {
        PeerLocator {
            designator: "peer".to_string(),
            transport: "tcp-testing-only".to_string(),
            hints: BTreeMap::from([
                ("host".to_string(), host.to_string()),
                ("port".to_string(), "22045".to_string()),
            ]),
        }
    }

    /// **The metadata endpoint is never dialed**, with no configuration: it is the classic SSRF
    /// target and it is never a CapTP peer.
    #[test]
    fn link_local_and_the_metadata_endpoint_are_refused() {
        let policy = DialPolicy::default();
        for host in ["169.254.169.254", "169.254.0.1", "fe80::1"] {
            let refused = policy.permits(&locator(host));
            assert!(refused.is_err(), "{host} must be refused");
            assert!(
                refused.expect_err("checked").contains("link-local"),
                "and the reason names why"
            );
        }
        assert!(policy.permits(&locator("0.0.0.0")).is_err());
    }

    /// **A remote peer cannot aim this node at its own loopback** (Law 62, AUDIT C225) — and a
    /// loopback peer still can, which is the control that keeps the rule from being "refuse local
    /// targets", a rule that would break every demo.
    ///
    /// The origin is the socket address the request came from. `None` — a transport that cannot say —
    /// keeps the behaviour it had, which is why it is asserted here too: the origin rule must not turn
    /// an unknowable origin into a refusal.
    #[test]
    fn a_remote_peer_cannot_reach_this_nodes_own_services() {
        let policy = DialPolicy::default();
        let remote = "203.0.113.9:41234".parse().expect("a remote address");
        for host in ["127.0.0.1", "::1", "192.168.1.10", "10.0.0.5"] {
            let refused = policy.permits_from(&locator(host), Some(remote));
            assert!(
                refused.is_err(),
                "a peer at {remote} must not make this node dial its own {host}"
            );
            assert!(
                refused
                    .expect_err("checked")
                    .contains("cannot make this node dial"),
                "and the reason says whose address it is"
            );
        }
        // A public target is still a dial the peer may ask for: the rule is about *this node's* local
        // addresses, not about who is asking.
        assert!(policy
            .permits_from(&locator("203.0.113.10"), Some(remote))
            .is_ok());

        // **The control.** A peer that is itself on loopback is dialling its own neighbourhood.
        let local = "127.0.0.1:5555".parse().expect("a loopback address");
        for host in ["127.0.0.1", "192.168.1.10"] {
            assert!(
                policy.permits_from(&locator(host), Some(local)).is_ok(),
                "a loopback peer reaching {host} is the conformance suite's own case"
            );
        }

        // And an origin the transport cannot report is judged as before, not refused.
        assert!(policy.permits_from(&locator("127.0.0.1"), None).is_ok());
    }

    /// The origin rule **cannot be evaded by a name**: the same resolution `permits` does is applied
    /// before the address is judged, so a name pointing at this node is refused for a remote peer.
    #[test]
    fn a_remote_peer_cannot_reach_this_node_by_name() {
        let policy = DialPolicy::default();
        let remote = "203.0.113.9:41234".parse().expect("a remote address");
        // `localhost` resolves to loopback on every host this runs on.
        let refused = policy.permits_from(&locator("localhost"), Some(remote));
        assert!(
            refused.is_err(),
            "a name that resolves to loopback is the same dial: {refused:?}"
        );
    }

    /// Loopback and private addresses are **permitted by default**, because that is how this crate is
    /// tested and how the ERTP transcript runs — and deniable, because a node serving a stranger has
    /// no business dialing its own LAN on that stranger's word.
    #[test]
    fn local_addresses_are_permitted_by_default_and_deniable() {
        let open = DialPolicy::default();
        for host in ["127.0.0.1", "::1", "192.168.1.10", "10.0.0.5"] {
            assert!(
                open.permits(&locator(host)).is_ok(),
                "{host} is the demo's case and must be permitted by default"
            );
        }

        let strict = DialPolicy {
            deny_local: true,
            allow: Vec::new(),
        };
        for host in ["127.0.0.1", "192.168.1.10", "10.0.0.5"] {
            assert!(
                strict.permits(&locator(host)).is_err(),
                "{host} must be refused"
            );
        }
        // A public address is still fine, and an explicit allowance overrides the policy.
        assert!(strict.permits(&locator("8.8.8.8")).is_ok());
        let allowed = DialPolicy {
            deny_local: true,
            allow: vec!["127.0.0.1".to_string()],
        };
        assert!(allowed.permits(&locator("127.0.0.1")).is_ok());
    }

    /// **A name is resolved before it is judged**, and one that does not resolve is refused rather
    /// than waved through: the dial would fail anyway, and checking only the spelling is the classic
    /// way to evade a policy like this one.
    #[test]
    fn a_name_is_resolved_and_an_unresolvable_one_is_refused() {
        let policy = DialPolicy::default();
        // `localhost` is the interesting case: it *resolves*, and what it resolves to is judged. With
        // local addresses permitted — the demo's default — it passes…
        assert!(
            policy.permits(&locator("localhost")).is_ok(),
            "localhost resolves to loopback, which is permitted by default"
        );
        // …and with them denied it does not, which is the whole point of resolving.
        let strict = DialPolicy {
            deny_local: true,
            allow: Vec::new(),
        };
        assert!(
            strict.permits(&locator("localhost")).is_err(),
            "a name that resolves to a denied address must be refused"
        );

        // A name nothing can resolve is refused rather than handed to the dialer.
        assert!(policy
            .permits(&locator("no-such-host.invalid"))
            .expect_err("unresolvable")
            .contains("does not resolve"));
    }

    /// **A `url` hint names a host, and the policy judges it.**
    ///
    /// The `websocket` transport carries its whole address in `url` rather than in `host`, so a policy
    /// that read only `host` returned `Ok` for every websocket target — the link-local and metadata
    /// refusals and Law 62's origin rule were skipped, and a sturdyref naming
    /// `ws://169.254.169.254/` was dialled. The HAZOP's red team measured exactly that (row E10); this
    /// is the test that would have caught it.
    #[test]
    fn a_websocket_target_is_judged_by_its_url() {
        let policy = DialPolicy::default();
        let ws = |url: &str| PeerLocator {
            designator: "peer".to_string(),
            transport: "websocket".to_string(),
            hints: BTreeMap::from([("url".to_string(), url.to_string())]),
        };

        // The metadata endpoint, named only by a url, is refused.
        assert!(
            policy.permits(&ws("ws://169.254.169.254/")).is_err(),
            "a url naming the metadata endpoint must be refused like any other"
        );
        // A bracketed IPv6 literal is read as an address, not as a host containing a colon.
        assert!(policy.permits(&ws("ws://[fe80::1]:9045/")).is_err());
        // And Law 62 applies through a url too: a remote peer may not aim this node at its own
        // loopback by naming it in a websocket address.
        let remote = "203.0.113.9:41234".parse().expect("a remote address");
        assert!(policy
            .permits_from(&ws("ws://127.0.0.1:9000/"), Some(remote))
            .is_err());
        // A public target is still a dial a peer may ask for.
        assert!(policy.permits(&ws("ws://203.0.113.10:9000/")).is_ok());
        // And a locator that names no host at all is still "cannot be judged", as `unix` requires.
        let unix = PeerLocator {
            designator: "peer".to_string(),
            transport: "unix".to_string(),
            hints: BTreeMap::from([("path".to_string(), "/run/peer.sock".to_string())]),
        };
        assert!(policy.permits(&unix).is_ok());
    }
}
