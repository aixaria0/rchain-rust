# The Noise interop run

**What this is.** The result of driving **Agoric's** Noise handshake against this repository's
`ocapn` netlayer (`ocapn/src/noise.rs`). The unit tests there handshake two of *our* endpoints with
each other, which proves the state machine and the framing agree with each other and proves nothing
about whether they agree with anyone else. This is the other side of the wire.

The reference is `rust/ocapn_noise` in [`endojs/endo`](https://github.com/endojs/endo), pinned at
commit `356d6e70affc5adfd35cd65adda758119521ec5f` (2026-10-03) and digested when fetched — see
`run-1.txt`. `@endo/ocapn-noise` is **unpublished** (the npm registry 404s on it) and described as
experimental, so "the reference" is a source revision and nothing more stable than one.

## How to reproduce

```sh
bash spec/audit/evidence/ocapn-noise/run.sh
```

The script fetches the reference at the pinned commit, makes **two mechanical edits** (below), builds
the harness, runs it, and writes `run-1.txt`.

**The two edits, stated because a transformed reference is not the reference.** Both are forced by
inlining the file rather than linking it, and the script refuses to run if either shape has changed:

1. **`#![no_std]` is dropped.** A crate-level attribute — meaningful only when that file *is* a crate
   root, and an error when inlined into a harness that needs `std` for its sockets.
2. **The `unsafe extern "C" { fn buffer_callback(…) }` declaration is dropped.** It is the *one* host
   symbol the reference needs; inlined into a single crate it collides with the definition the harness
   supplies (`E0428`). The *calls* to `buffer_callback` are left exactly as they are, and they resolve
   to that definition — the script checks both that the declaration went and that the calls stayed.

Nothing else in the reference is touched, and its digest is recorded so a later reader can tell
whether the pin still resolves to the text that was run.

## What a run proves, and what it does not

**Proves:** the handshake completes across implementations — the reference accepts our SYN, our ACK,
and the signature checks on both sides hold — and transport messages decrypt in both directions.

**Does not prove: the record framing.** The reference's `encrypt`/`decrypt` work over one record of at
most 65535 bytes and leave the record *boundaries* to a netlayer. `@endo/ocapn-noise` ships no
netlayer, so the length prefix and chunking in `ocapn/src/noise.rs` have no counterpart to be tested
against. That part of the transport is our own design, and the transcript says so rather than letting a
green run imply more.

**Does not prove: a live peer.** This drives the reference's *core* from a harness, which is what the
reference is built to be driven by (its own JS binding does the same thing). It is not the same as
talking to a shipped implementation, and none exists to talk to.

**Does not cover the designator convention.** Endo names a peer by a base32 public key; this harness
passes the responder's Ed25519 verifying key in the locator's `verify` hint (base16), because the
locator convention is not pinned by anything reachable. See the plan's decision 4.


## The record framing is unobservable upstream — C227's close (read 2026-10-07)

Three facts, each read from the source rather than inferred:

1. **Upstream has not moved.** `endojs/endo` master's `rust/ocapn_noise/src/lib.rs` is `noise_xx` with an
   empty prologue and the payload of a verifying key plus a signature — the shape of the pinned
   `356d6e70` this directory's `run-1.txt` was taken against — and upstream's `packages/ocapn-noise`
   describes itself as the "**XX** … variant with **Ed25519 signature verification**". This port matches
   the reference on the pattern, the prologue, the payload and the message count.
2. **Upstream still ships no netlayer.** `packages/ocapn-noise/src/` upstream contains `bindings.js`
   alone — no transport, no session layer, nothing that could decide a record boundary. "What a run
   proves, and what it does not" above therefore still holds, and for the same reason: there is **no
   counterpart to test this port's length prefix and chunking against**.
3. **The only netlayer that exists is a proposal branch's.** `endojs/endo-but-for-bots`, branch `llm`,
   carries `packages/ocapn-noise/src/transports/{mock,tcp,ws}` and a session layer whose TCP transport
   frames with **netstring** (`@endo/netstring`, `MAX_FRAME_LENGTH = 65_551`) and whose session layer
   maps **one frame to one OCapN message**, with no chunking and no maximum-message check of its own.
   But that branch is also a **rewrite of the handshake**: `noise_ik`, no per-message signature,
   `prologue = b"OCapN/np/1\0" || responder key`, and an X25519 static derived from the Ed25519 seed.
   The framing and the pattern arrive together, which is why neither can be adopted alone.

**So the boundary is not pinnable by observation today** — not because nobody looked, but because the
reference publishes no boundary to pin to. Nothing in this port changes.

**The alignment, named with its trigger.** Every *other* OCapN TCP transport frames with netstring: the
Python suite (`ocapn-test-suite/utils/netstrings.py`, which `ocapn/src/netstring.rs` already implements
for `tcp-testing-only`), Endo's own `tcp-test-only` netlayer, and now the `llm` branch's noise netlayer.
This transport is the only one that does not. **When a netlayer lands upstream**, the change is:
`crate::framed::Framed` in `ocapn/src/noise.rs` for the handshake and the session, the chunking dropped,
and a message bounded at ~65 519 bytes — the reference's own `SIZE = 65535`. That is a **capability
reduction** on this transport (4 MiB today) and is why it is not made speculatively.

**And a convention this confirms rather than changes.** The `llm` branch names a peer by the hex
Ed25519 key (64 characters) — what this port already uses for `noise`, and what C245 changed for
`websocket` (base32). The two transports genuinely differ in that convention, and both now match the
peers that dial them.
