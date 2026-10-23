# #420 — Finalized fringe refusal diagnostics

## Scope

The responder cannot safely substitute a generic finalized/genesis root when the requester explicitly named an unavailable recovery anchor. It must continue to **withhold** a success response in that case. The issue reports missing answers during severe resource pressure, but that does not establish which of transport, handler, block-store or scheduler caused each missing reply.

This change **does not modify protocol messages, fork choice, Casper CBC, root selection or bootstrap peer configuration.** It makes existing fail-closed refusal observable:

- A received `FinalizedFringeRequest` is logged with peer, requested anchor or ordinary marker, and `include_fringe_metadata`.
- A requested block absent from the local `BlockStore` is logged with its hash as a missing anchor, distinct from an actual store read error (already logged).
- If neither latest fringe nor genesis is known, an ordinary request has a distinct log reason.
- When no reply is sent, the responder logs the peer/anchor refusal. On success, the existing reply path is unchanged.
- An unknown anchor never silently falls back to an unrelated state root.

## Acceptance

~~~sh
cargo fmt --all -- --check
cargo test -p rchain-casper --lib an_anchored_fringe_request_is_answered_with_that_block_and_its_ancestry
~~~

The pinned regression asserts both absence of a response and a logged missing-anchor reason. It is not an end-to-end transport test.

## Incident discriminator

Correlate timestamped joiner **send outcome** and responder receipt/refusal logs, exact source revision, endpoint and anchor:

| Client log | Responder receipt | Responder reason | Interpretation |
| --- | --- | --- | --- |
| Send timeout | none | none | Transport/dial/peer starvation remains plausible; do not infer handler fault |
| Sent | none | none | Unobserved responder path; request receipt not confirmed |
| Sent | yes | missing anchor | Fail-closed reply suppression; operator should verify anchor availability |
| Sent | yes | BlockStore read error | Storage error, not valid empty state |
| Sent | yes | sent success | Investigate transport of reply or joiner validation |

These are **diagnostic distinctions**, not proof of OOM causality. Repeated warning logs are not a rate-independent health metric.

## Excluded changes

A structured negative reply needs a versioned wire-protocol decision and requester compatibility handling. Multiple bootstrap peers need tested candidate selection and anchored-root agreement, not blind fallback. Memory preflight, cgroup-aware shedding and source-pinned Root replay are separate tracks; HTTP admission Draft #426 and read-only evidence Draft #421 address different scopes.

No live validator is restarted, resized, loaded or otherwise changed by this proposal.
