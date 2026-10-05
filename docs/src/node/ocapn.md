# OCapN interoperability

> This page is the decision record for RNode's **OCapN** support — the object-capability network
> (`https://ocapn.org/`) that Agoric's stack speaks, so a vat on Agoric can hold a live reference
> to an RChain object and invoke it, and vice versa. It is **Layer 2** of the cross-shard design
> record: [](shard-invoke.md) fixed Layer 1 (a cross-shard call is a caller-signed deploy) and
> named this layer, promise pipelining, and three-party handoff as out of scope there. This page
> takes that scope up.

**Status.** Only the first slice is built — the wire codec and the locators (below). Everything
under *The build* other than that slice is **proposed and not yet implemented**; the staging is the
OCapN implementation guide's own six stages, and each is listed with its state.

## What OCapN is, in one paragraph

CapTP is a capability transport: two peers establish a *session* over a pluggable **netlayer** and
then exchange `op:deliver` messages addressed to *import/export descriptors* rather than to
hostnames. A peer's objects are reached through **sturdyrefs** — a peer locator plus a swiss number
(`<ocapn-sturdyref peer swiss-num>`) — that the receiving peer resolves against its *bootstrap
object*, always exported at position 0. Messages may name a not-yet-resolved result (**promise
pipelining**), which OCapN answers with `desc:answer` and settles with `fulfill`/`break`. CapTP
messages are carried in **Syrup**, a Preserves-family binary encoding.

## The build

The crate is `ocapn/` (package `rchain-ocapn`), a workspace member. It is node-local: sessions,
wire bytes, and answer bookkeeping never reach consensus. What reaches consensus is only the signed
deploy the bridge produces (below).

| Stage | Deliverable | State |
|---|---|---|
| 0a | Syrup codec + locators | **built** — `ocapn/src/{syrup,locator,peer}.rs` |
| 0b | Netlayer trait, `tcp-testing-only` netlayer, `op:start-session`, `op:abort` | proposed |
| 1 | Import/export tables, `op:deliver`, bootstrap object at position 0, sturdyref `fetch` | proposed |
| 2–5 | Promises/answers, `op:listen`, pipelining, GC (`gc-exports`/`gc-answers`) | proposed |
| 6 | Third-party handoffs (Gifter / Receiver / Exporter) | proposed |
| — | The bridge: an `op:deliver` to a chain-backed export becomes a signed deploy | proposed |

### Built: the codec and the locators

`ocapn/src/syrup.rs` implements the concrete Syrup grammar of
`draft-specifications/Notation.md`: booleans (`t`/`f`), arbitrary-precision integers
(`42+`, `1-`), float64 (`D` + 8 network-order bytes), strings (`5"twine`), symbols
(`12'fleur-de-lis`), byte arrays (`8:`), lists (`[…]`), string-keyed structs (`{…}`), and records
(`<…>`). Encoding is canonical — struct keys sorted, no leading zeros, no inter-token whitespace.
Decoding refuses rather than panics or truncates: a trailing byte, a length that overruns the
buffer, non-UTF-8 where a string is required, a duplicate struct key, or nesting past `MAX_DEPTH`
(a recursive descent over network bytes is a stack-depth DoS unless it is bounded) are all errors.
Known-answer tests pin the byte patterns, because a codec that round-trips but orders itself
differently from a peer is the one interop failure that never shows up locally.

`ocapn/src/locator.rs` and `ocapn/src/peer.rs` implement the two locators of
`draft-specifications/Locators.md`, in both their forms: the out-of-band URI
(`ocapn://<designator>.<transport>[/s/<swiss-num>][?hints]`) and the in-band Syrup record
(`<ocapn-peer …>`, `<ocapn-sturdyref …>`). A designator may contain dots — the *trailing* dot is
the designator/transport separator — and the swiss number is an opaque string, never parsed as a
number.

**What the draft leaves undecided is not invented here.** OCapN's `Undefined`, `Null`, `Tagged`,
`Reference`, and `Error` values have no concrete Syrup form in `Notation.md`, and general Syrup
permits non-string dictionary keys where an OCapN *Struct* does not. Both are left open until the
message layer needs them, rather than guessed.

## Invariants

- **Nothing here is consensus state.** Session keys and ids, wire bytes and framing, swiss-num
  transport, handoff gift-ids and counters, and the `gc-*` bookkeeping are node-local. The only
  chain-visible object is the caller-signed deploy the bridge submits — the same object Layer 1
  already defines.
- **Session identity and chain identity are two layers, never merged.** A CapTP session key is
  **Ed25519** and off-chain; the node's OCapN *designator* is the **secp256k1** deployer key.
  Ed25519 is deliberately disabled as an on-chain signature algorithm
  (`crypto/src/signatures/signatures_alg.rs`, RCHAIN-3560) and stays disabled; the binding between
  a session key and a deployer key is a signed statement carried in the session, not a change to
  the deploy's signature scheme.
- **A CapTP promise can be finalised by consensus** — but finalisation means "a finalised block
  committed the value on the reply channel", not a language-level future. Pending answers,
  timeouts, and breaks are session state; a contract that acts on an answer must be idempotent per
  deploy id.

## Related

- [Cross-shard invoke](shard-invoke.md) — Layer 1, the primitive this is built on.
- [`spec/RUST-FIRST.md`](../../../spec/RUST-FIRST.md) — the native-state model the ERTP layer (the
  payload these capabilities carry) joins.
