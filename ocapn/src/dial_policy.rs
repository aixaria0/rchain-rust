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
//! **Not addressed, and named rather than implied:** a *hostname* is checked by name, not by what it
//! resolves to, so a name that resolves into a denied range gets through (DNS rebinding); and a remote
//! peer can still aim the node at the node's own loopback services when `deny_local` is off. Both need
//! the peer's own origin — the socket address of the session it arrived on — which the dial path does
//! not carry. That is the registered refinement, not something this file pretends to solve.

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

    /// Whether a dial to `locator` is permitted, or why it is not.
    pub fn permits(&self, locator: &PeerLocator) -> Result<(), String> {
        let Some(host) = locator.hints.get("host") else {
            // A transport whose hints carry no host cannot be checked, and refusing everything that
            // lacks a hint would break every netlayer that reaches its peer another way.
            return Ok(());
        };
        if self.allow.iter().any(|a| a == host) {
            return Ok(());
        }
        let Ok(address) = host.parse::<std::net::IpAddr>() else {
            // A name: checked by name, not by resolution (see the module note).
            return Ok(());
        };
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
        // **Refused before the connect, not after.** The point is to not make the connection: a
        // policy that dials and hangs up has already told the peer whether something is listening.
        self.policy
            .permits(locator)
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

    /// A **name** is checked by name, not by what it resolves to — the gap the module note names
    /// rather than hides.
    #[test]
    fn a_hostname_is_permitted_and_that_is_a_known_gap() {
        let strict = DialPolicy {
            deny_local: true,
            allow: Vec::new(),
        };
        assert!(
            strict
                .permits(&locator("metadata.example.internal"))
                .is_ok(),
            "a name is not resolved here, so a name that resolves into a denied range gets through"
        );
    }
}
