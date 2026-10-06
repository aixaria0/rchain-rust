import Rchain.Par

/-!
# Law 62 — a peer cannot extend this node's reach

The peer chooses the address a CapTP dial goes to: a sturdyref carries a peer locator and a handoff
give carries an `exporter-location`, and both end at the netlayer's dial. A node that judges the
*target* and not the *peer* is an SSRF primitive — the classic target being the cloud metadata
endpoint, and the useful one a scan of the operator's LAN (the HAZOP's row B4, measured: the
attacker's server received our `op:start-session`).

The policy already refused link-local and the unspecified address outright, resolved a name before
judging it, and could refuse loopback and private ranges on request. **What it could not do is tell
one peer from another** (AUDIT C225): with `deny_local` off — which is the default, because the
conformance suite and the ERTP transcript both run over loopback — a *remote* peer could aim this node
at its own loopback services. The fix threads the peer's own origin, the socket address of the session
the request arrived on, into the policy, so the rule becomes: **the node never dials, on a peer's
word, anything that peer could not dial itself.**

**Why the rule is a containment rather than a list.** A list of denied targets keyed to nothing is
what the policy already had, and it is the shape that let the C225 case through: the same target is
legitimate for one peer and an escalation for another. Stating the law as a containment — the node's
reach under a peer is *within* the peer's own reach — is what makes the origin part of the statement
rather than a parameter of it. The rule the fix replaces is modelled beside it as `originBlind`, and
the pair of theorems is the claim.

Layer `Wire`; the Rust is `ocapn/src/dial_policy.rs` (`permits_from`, `not_a_local_target`),
`ocapn/src/netlayer.rs` (`NetConn::peer_address`, `Netlayer::new_outgoing_connection_from`) and the
two dial sites that now say which peer asked (`ocapn/src/enliven.rs`, `ocapn/src/fixtures.rs`).
-/

namespace Rchain

/-- Where a peer is, as far as the rule is concerned: on this host, or not. That is the whole
distinction the socket address gives, and it is enough for the rule. -/
inductive Origin where
  /-- The peer's address is one of this host's own. -/
  | onHost
  /-- The peer is somewhere else. -/
  | elsewhere
deriving DecidableEq, Repr

/-- Where a dial points, as far as the rule is concerned: at this node's own services, at something on
its own network, or at the public internet. -/
inductive Target where
  /-- Loopback: this node's own services. -/
  | ourselves
  /-- A private range: this node's own network. -/
  | ourNetwork
  /-- A public address. -/
  | public
deriving DecidableEq, Repr

/-- What a peer can reach **on its own**, without this node's help. A peer on this host reaches
everything on it; a peer elsewhere reaches only public addresses. -/
def ownReach : Origin → Target → Bool
  | .onHost, _ => true
  | .elsewhere, .public => true
  | .elsewhere, _ => false

/-- Whether a target is one of **this node's own** services or networks, as opposed to a public
address. -/
def isLocal : Target → Bool
  | .ourselves => true
  | .ourNetwork => true
  | .public => false

/-- Whether this node may dial `target` on the word of a peer at `origin` — the policy's rule, built
from its **ingredients** rather than from a table: a target in the always-refused set (link-local, the
metadata range, the unspecified address); a target the operator denied local ones for; and a target
this node's own, when the peer is elsewhere. -/
def permits (alwaysRefused : Target → Bool) (denyLocal : Bool) (o : Origin) (t : Target) : Bool :=
  !alwaysRefused t && !(denyLocal && isLocal t) && !(o == Origin.elsewhere && isLocal t)

/-- **Law 62** — the node never dials, on a peer's word, anything that peer could not dial itself. The
statement is a **containment**, for *every* configuration of the policy's ingredients: the node's reach
under a peer is inside the peer's own reach. Refusing more can only shrink the node's side; the term
that makes it true where it would otherwise fail is the **origin** — which is what the fix added, and
what `originBlind` below is missing. -/
theorem the_node_never_exceeds_the_peers_own_reach
    (alwaysRefused : Target → Bool) (denyLocal : Bool) (o : Origin) (t : Target) :
    permits alwaysRefused denyLocal o t = true → ownReach o t = true := by
  cases o <;> cases t <;> simp [permits, ownReach, isLocal]

/-- The shape the fix replaces: the target is judged **without the origin**, which is what the policy
did before the peer's socket address was threaded to it. `denyLocal` is the operator's switch — off by
default, because a loopback peer is how this crate is demonstrated. -/
def originBlind (denyLocal : Bool) : Target → Bool
  | .ourselves => !denyLocal
  | _ => true

/-- **The falsifier, and it is the C225 case.** Under the origin-blind rule, with the **default**
configuration, a peer elsewhere reaches this node's own services — reach the peer itself does not
have. The pair of this with the law above is what makes the law about the rule rather than a restating
of the code: the fix is not a new list of targets, it is the origin entering the judgement. -/
theorem the_origin_blind_rule_exceeds_the_peers_reach :
    originBlind false .ourselves = true ∧ ownReach .elsewhere .ourselves = false := by decide

/-- And the operator's switch alone is not the law: denying local targets refuses them for a peer on
this host too, which is a *demo* broken rather than an escalation closed — the reason the fix keys on
the origin instead. -/
theorem denying_local_targets_outright_would_refuse_a_local_peer :
    originBlind true .ourselves = false ∧ ownReach .onHost .ourselves = true := by decide

end Rchain
