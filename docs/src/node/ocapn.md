# OCapN interoperability

> This page is the decision record for RNode's **OCapN** support — the object-capability network
> (`https://ocapn.org/`) that Agoric's stack speaks, so a vat on Agoric can hold a live reference
> to an RChain object and invoke it, and vice versa. It is **Layer 2** of the cross-shard design
> record: [](shard-invoke.md) fixed Layer 1 (a cross-shard call is a caller-signed deploy) and
> named this layer, promise pipelining, and three-party handoff as out of scope there. This page
> takes that scope up.

**Status.** Stages 0–2 are built — the wire codec and locators, the session identity, the netlayer,
the CapTP connection with its tables, and `op:deliver` with promises, pipelining, `break`, and
`op:listen` — plus `op:gc-exports` from stage 3. They are checked against the OCapN conformance
suite: **15 of 24 tests pass**, all in the implemented path. `op:gc-answers`, handoffs, and the
bridge to the chain are **proposed and not yet implemented**; the staging is the implementation
guide's own six stages, and each is listed with its state.

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
| 0b | Session identity: `op:start-session`, Public Identifier, Session ID, crossed hellos | **built** — `ocapn/src/{session,session_id}.rs` |
| 0c | Netlayer trait, `tcp-testing-only` netlayer, `op:abort` | **built** — `ocapn/src/{netlayer,tcp_testing_only}.rs`, `session.rs` |
| 1 | Import/export tables, `op:deliver`, the bootstrap at position 0, `fetch` | **built** — `ocapn/src/{captp,conn,bootstrap,fixtures}.rs` |
| 2 | Promises and answers: `fulfill`/`break` via `resolve-me-desc`, pipelining via the answer table, `op:listen` | **built** — `ocapn/src/{conn,captp,fixtures}.rs` |
| 3 | GC: `op:gc-exports` and the wire-delta accounting | **built** — `ocapn/src/{captp,conn}.rs` |
| 3 | GC: `op:gc-answers` | proposed |
| 4–5 | Pipelining refinements, `resolve-me-desc` folding, `op:gc-answers` | proposed |
| 6 | Third-party handoffs (Gifter / Receiver / Exporter) | proposed |
| — | The bridge: an `op:deliver` to a chain-backed export becomes a signed deploy | proposed |

### Checked against the reference suite

`ocapn-tcp-testing` (`ocapn/src/bin/`) serves the suite's fixture objects, and the suite has been
run against it: **15 of 24 tests pass**, all of them in the implemented path — `op_abort` 1/1,
`op_deliver` 4/4 (including both promise-pipelining tests and the break-propagation test),
`op_listen` 3/3 (the promise/resolver pair, heard before and after the settlement), `op_gc` 3/4
(the wire-delta accounting; the fourth needs the greeter to hand out a resolver), and
`op_start_session` 3/5 (the two failures need the sturdyref enlivener, which is not built). Handoffs
(1/7, incidentally) are the unimplemented stage, and the suite says so. The runs,
the suite revision, and the per-module counts are kept in
[`spec/audit/evidence/ocapn-conformance/`](../../../spec/audit/evidence/ocapn-conformance/README.md)
so a later run can be compared against them.

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

**Checked against the reference, not just against itself.** `ocapn/tests/reference_vectors.rs`
pins the encodings of the real message shapes — the peer record, `op:deliver`, `op:start-session`,
and the signed `<my-location …>` payload — to vectors produced by the suite's own `syrup_encode`. It
was that check that surfaced three divergences, all now fixed: struct members must be ordered by
their **encoded** key bytes rather than the key string (the reference's `sorted(key=syrup_encode)`,
which differs as soon as two keys have different lengths, and the session signature covers a struct);
a peer's hints are always a **struct**, so an empty hint set is `{}`, not the grammar's `f`; and the
**swiss number is a byte array**, not the "string" `Locators.md` calls it.

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

### Built: the session identity

`ocapn/src/session_id.rs` is the spec's six-step derivation and nothing else — the Public
Identifier is two SHA-256 rounds over the serialized session public key, and the Session ID is
`SHA256(SHA256("prot0" ‖ sorted(PI_a, PI_b)))`, sorted by octets so two peers agree without
agreeing on who is "first". The crossed-hello rule aborts the *lower* Public Identifier. The
constants are pinned by known-answer vectors against an independent computation, because the
composition (sort order, the `prot0` prefix, the round count) is the part that interoperates with
nobody while passing every local round-trip.

`ocapn/src/session.rs` carries the `op:start-session` message. **Here the prose is wrong and the
reference implementation is the oracle**: the CapTP draft gives the operation five fields, including
a `crypto-version` it then contradicts itself about; the OCapN test suite's `OpStartSession` carries
four — `captp_version`, `session_pubkey`, `location`, `location_sig` — with no `crypto-version` on
the wire at all, so a port built from the prose would fail every handshake against Endo, Goblins,
and DObjects. The port follows the implementation, and also reproduces the two fields that are
gcrypt s-expressions rather than raw bytes: the session public key
(`['public-key ['ecc ['curve 'Ed25519] ['flags 'eddsa] ['q …]]]`) and the signature
(`['sig-val ['eddsa ['r …] ['s …]]]`), over the payload `<my-location <locator>>`. Both shapes are
pinned by byte-level known-answer tests. (AUDIT C216.) `op:abort` is carried too — "the reason text
is the peer's to choose".

### Built: the netlayer

`ocapn/src/netlayer.rs` fixes the two functions the netlayer standard names,
`new_outgoing_connection(ocapn_locator)` and `accept_incoming_connection()`, over a channel that is
"a bidirectional FIFO": one message is one Syrup value, and CapTP above sees only a queue.
`ocapn/src/tcp_testing_only.rs` is the conformance suite's transport of the same name, implemented
as the suite describes it — raw TCP, no encryption, which its README flags as "HIGHLY INSECURE, DO
NOT USE IN PRODUCTION". **The boundary is a netstring, not bare Syrup**: the prose says the netlayer
"streams pure Syrup-encoded data directly", but the suite's `CapTPSocket.send_message` wraps every
message in a `Netstring` and its reader takes one netstring at a time — the third prose-versus-
implementation gap of its kind (AUDIT C216). `ocapn/src/netstring.rs` implements that framing, which
is `<ascii-decimal length>:<payload>` with **no trailing comma**. Two bounds keep an adversarial
stream harmless — the codec's nesting depth, and a message-size cap — and a stream that ends inside a
message is an error, never a silently dropped message. A production netlayer (Tor, libp2p, IBC)
implements the same two functions; nothing above the trait changes.

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
