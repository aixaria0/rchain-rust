# OCapN interoperability

> Requested by **Dan Connolly** of Agoric in the Rho Vision Colab Discord; the provenance and the
> close condition are [issue #249](https://github.com/rchain-community/rchain-rust/issues/249).
>
> This page is the decision record for RNode's **OCapN** support — the object-capability network
> (`https://ocapn.org/`) that Agoric's stack speaks, so a vat on Agoric can hold a live reference
> to an RChain object and invoke it, and vice versa. It is **Layer 2** of the cross-shard design
> record: [](shard-invoke.md) fixed Layer 1 (a cross-shard call is a caller-signed deploy) and
> named this layer, promise pipelining, and three-party handoff as out of scope there. This page
> takes that scope up.

**Status.** **Stages 0–6 are built, and the whole OCapN conformance suite passes: 24 of 24.** That is
the wire codec and locators, the session identity, the netlayer, the CapTP connection with its
tables, `op:deliver` with promises, pipelining, `break` and `op:listen`, the GC accounting in both
directions, the **sturdyref enlivener** (which dials a peer back from a sturdyref it is handed — the
dial-out case, and the missing half of the crossed-hello rule) and **third-party handoffs** in all
three roles. The **bridge to the chain** is built too: an `op:deliver` to a chain-backed capability
becomes a signed deploy, and a call that *returns* capabilities hands the peer a descriptor per
capability. Two foreign implementations have spoken to it: the suite above, and Agoric's own
`@endo/ocapn`, which dials a **node** and holds an RChain ERTP issuer (see
[`spec/audit/evidence/endo-spike/`](../../../spec/audit/evidence/endo-spike/README.md)).

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
| 3 | GC: `op:gc-exports`, `op:gc-answers`, and the wire-delta accounting | **built** — `ocapn/src/{captp,conn}.rs` |
| 4–5 | Pipelining refinements, `resolve-me-desc` folding, `op:gc-answers` | **built** — `ocapn/src/{conn,captp}.rs` |
| 6 | Third-party handoffs (Gifter / Receiver / Exporter), and the sturdyref enlivener that dials out | **built** — `ocapn/src/{owner,enliven,handoff,proxy,bootstrap}.rs` |
| — | The bridge: an `op:deliver` to a chain-backed export becomes a signed deploy | **built** — `node/src/api/ocapn.rs`, `casper/src/shard_invoke.rs` |

### Checked against the reference suite

`ocapn-tcp-testing` (`ocapn/src/bin/`) serves the suite's fixture objects, and the suite has been
run against it: **24 of 24 tests pass** — `op_abort` 1/1, `op_start_session` 5/5 (including both
crossed-hello variants), `op_deliver` 4/4 (including both promise-pipelining tests and the
break-propagation test), `op_listen` 3/3 (the promise/resolver pair, heard before and after the
settlement), `op_gc` 4/4 (the wire-delta accounting and `op:gc-answers`), and
`third_party_handoffs` 7/7 (Gifter, Receiver and Exporter, including the replay and
forged-signature refusals). The runs,
the suite revision, and the per-module counts are kept in
[`spec/audit/evidence/ocapn-conformance/`](../../../spec/audit/evidence/ocapn-conformance/README.md)
so a later run can be compared against them.

### Also checked against Agoric's own implementation

The Python suite is the OCapN project's reference; **`@endo/ocapn` is Agoric's**, the stack the
request came from. It has been run against `ocapn-tcp-testing` too: a peer on Node 22 completed the
handshake, `fetch`ed a sturdyref, **called the object and got a reply** — a string, a bigint and a
boolean, correctly typed. The transcript is
[`spec/audit/evidence/endo-spike/`](../../../spec/audit/evidence/endo-spike/README.md).

That spike also settled the wire's last open question and found a divergence:

- **Framing.** OCapN's TCP-for-testing netlayer is *described* as raw Syrup with no length prefix,
  and Endo keeps a `framing: 'none'` mode for that described wire while defaulting to `'syrup'`
  (`<length>:<payload>`). Only `'syrup'` works — against this port **and** against the Python suite,
  whose netlayer netstrings too. So `'none'` interoperates with nothing, and this port's netstring
  framing is right.
- **The swiss number's type.** Endo sends a *string* (as the Locators draft says); the Python suite
  sends a *byte array*. **The two reference implementations disagree with each other**, so "the
  reference implementation is the oracle" has no single oracle for this field — `Bootstrap::deliver`
  accepts both and keys its directory by bytes. Recorded as AUDIT C217.

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

### What the first pass left, and how it was closed

Each of these was a *named* boundary rather than an omission, and each was a prerequisite of the next.
All three are now built; the shape of each closure is worth keeping, because it is where the design
record was wrong:

- **The sturdyref enlivener** (`gi02I1qghIwPiKGKleCQAOhpy3ZtYRpB`) — the fixture that dials a peer
  back from a sturdyref and returns a live reference. It needed **dial-out**: a session the crate
  drives in the background, and an object that can address it after the delivery that created it has
  returned. Closed by `ocapn/src/owner.rs` — a `SessionHandle` (what a non-owning task may ask) plus
  a `SessionLoop` (the task that owns the socket and both tables) — and by `proxy.rs`'s `Forward`,
  which is how an object fetched on one session is handed to a peer on another (a descriptor is
  per-session, so it cannot be passed through; the forwarding export can).
- **Third-party handoffs** (stage 6) — the same ownership, plus gift signing and the replay counter.
  `handoff.rs` has the three records, the `Envelope` whose signature covers the object's **syrup
  encoding**, and the gift store; `bootstrap.rs` plays the Exporter's `deposit-gift`/`withdraw-gift`
  and `fixtures.rs`'s greeter plays the Receiver. The store is keyed by **(gift id, the gifter's
  session)**: keyed by gift id alone, two independent handoffs sharing `b"my-gift"` shared a replay
  guard, which is a defect the suite caught.
- **The bridge to the chain** — a sturdyref resolving to a Rholang capability, and a delivery to it
  becoming a caller-signed deploy. The `Par` ↔ Syrup translation is `ocapn/src/par_value.rs` (a
  *partial* map: a Symbol has no Rholang counterpart inbound, and an unforgeable name is refused
  outbound rather than copied). Two things the plan did not anticipate: a capability a contract
  **returns** has no source literal, so the deploy that produces it must register it in the same
  evaluation (`shard_invoke.rs`'s `invoke_member_term`, via `rho:registry:insertArbitrary` — *not*
  `insertSigned`, whose URI comes from the deployer key and so cannot be signed by a key with no
  REV); and a **tuple crosses Syrup as a list**, not as a record, because records are labelled and
  `(true, 0)` has no legal label.

## Invariants

*(Corrected 2026-10-05 by the HAZOP at `spec/audit/evidence/ocapn-hazop.md`. Two of the three below
were **false**: they described the state the design intends rather than the state the code is in, and
a design page that does that is worse than one that says nothing, because it is the page an operator
reads to decide what the listener exposes.)*

- **Sessions, wire bytes and handoff bookkeeping are node-local; a bridged delivery is not.** The
  caller-signed deploy is the only thing CapTP *itself* puts on chain — but a delivery to a
  **method-carrying** capability also runs `rho:registry:insertArbitrary` in that same deploy, so it
  mints a **permanent** registry entry holding the reply (capabilities included), and it spends the
  node's own REV as phlo. That entry is consensus state on every node that replays, it is unbounded
  in count, and nothing deletes it (AUDIT C221).
- **Session identity and chain identity are two layers, and they are not merged *today*.** A CapTP
  session key is **Ed25519**, ephemeral (`Identity::fresh`) and off-chain; Ed25519 stays disabled as
  an on-chain signature algorithm (`crypto/src/signatures/signatures_alg.rs`, RCHAIN-3560). **But
  there is no binding between them yet**: every bridged deploy is signed by the one
  `dev.deployer-private-key`, so the chain sees the *node* as the caller for every peer. The design's
  intent — a signed statement in the session that a deployer key stands for this session — is
  **future work**, and the node's OCapN designator is the literal string `"rnode"`, not that key.
  Until the binding exists, `api-server.ocapn-listen` must be treated as **publishing the node's own
  authority**, not as admitting identified callers.
- **A CapTP promise can be finalised by consensus** — but finalisation means "a finalised block
  committed the value on the reply channel", not a language-level future. Pending answers,
  timeouts, and breaks are session state; a contract that acts on an answer must be idempotent per
  deploy id.

## Related

- [Cross-shard invoke](shard-invoke.md) — Layer 1, the primitive this is built on.
- [`spec/RUST-FIRST.md`](../../../spec/RUST-FIRST.md) — the native-state model the ERTP layer (the
  payload these capabilities carry) joins.
