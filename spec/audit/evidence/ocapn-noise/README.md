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
