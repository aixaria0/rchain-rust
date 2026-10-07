# OCapN transports — HAZOP and root-cause analysis

**Reviewed revision.** Branch `ocapn/noise`, its eight commits (`6cb5fcf7b` … `fc0ebea88`) on top of
`f9f8ca3ab`. It adds two OCapN transports to the node — `noise` (TCP under a Noise `XX` handshake) and
`websocket` (the transport `@endo/ocapn` speaks) — with a new X25519 primitive, an **identity file**,
a **designator that moved**, and a change to the accept loop. Fixes from this study land on the same
branch; the rows below say which.

**Why this study exists.** The prior HAZOP (`spec/audit/evidence/ocapn-hazop.md`) asked what a
*stranger* can do to the OCapN surface — but the surface it examined was `tcp-testing-only` and
`unix`. Two transports now authenticate, one of them cryptographically, and the node has an identity it
did not have. "Does the handshake work" is the conformance question; this is the other one.

**Method.** The same instrument, reused rather than reinvented: six lenses (**protocol, concurrency,
resources, authority, operations, verification**) + a **red team that ran its attacks against a live
node** + a **steelman** + adjudication. Guide words per unit: `No/None, More, Less (incl. part of), As
well as, Reverse, Other than, Early, Late`, each a row, folded, or vacuous **with a reason**. Worksheet
shape and scales are the house ones (`docs/src/spec/testnet-acceptance.md` §1; severity S1–S4,
likelihood L-A–L-D). Row ids are letter-grouped by lens.

**What this study's process cost and what it bought.** Ten agents ran; eight completed. The steelman
and one adjudicator were **blocked by a safety classifier** in the first pass and were re-run singly.
That mattered: the steelman **disagreed with the adjudicator on eight row groups**, and the
disagreements are adjudicated in §6 rather than averaged away. A study that had shipped the first
pass would have carried eight rows nobody had argued against.

## Units, with their design intent

| node | what it is | intent |
|---|---|---|
| **N1** | the Noise handshake and framing (`ocapn/src/noise.rs`) | a peer proves the key it dialled and the channel is bound to it |
| **N2** | the WebSocket upgrade and in-band challenge (`ocapn/src/websocket.rs`) | the server proves the key the dial named; the upgrade is bounded |
| **N3** | the identity file and key material (`load_or_create_noise_identity`, `NoiseIdentity`, `crypto/src/encryption/x25519.rs`) | one stable node identity, mode `0600`, never a zero key |
| **N4** | the listener set and the accept loop (`node/src/api/ocapn.rs`) | one bad connection cannot end a listener, and no peer starves it |
| **N5** | the per-transport bounds and timeouts | every peer-controlled wait and attacker-sized buffer is bounded |
| **N6** | the dial perimeter (`dial_policy.rs`, `peer_address`) | Law 62's origin rule holds wherever a transport reports an origin |
| **N7** | the CapTP session above them (`conn.rs`, `owner.rs`, `bootstrap.rs`) | a delivery over either transport is dispatched and answered |
| **N8** | the operational surface (config, logs, restart, failure) | an operator can tell "serving" from "silently dead" |

## §1 The hazard inventory (the study's input)

Facts the red team **measured**, listed before any deviation is judged.

| id | hazard | the sharpest fact | how known |
|---|---|---|---|
| **H1** | a short frame panics a session task | a 1..=15-byte framed body reached `decrypt_in_place`'s `assert!(ciphertext_len >= 16)`; reproduced at lengths 8 and 15 | **measured** |
| **H2** | the handshake read is unbounded and signs what it reads | 8 MiB `init:peer-auth` payload: buffered, Ed25519-signed, and echoed — 8,388,776 B back, +24.5 MB RSS | **measured** |
| **H3** | 64 bare sockets take the whole ceiling | every permit held by connections that sent **zero bytes**; the 65th refused on *every* transport | **measured** |
| **H4** | the websocket dial policy is blind | with `ocapn-deny-local-dial = true`, tcp refused loopback and `169.254.169.254`; websocket **connected** to loopback and attempted the metadata address | **measured** |
| **H5** | a pending websocket accept is cancelled by other traffic | a ws peer mid-challenge is reset the instant a connection lands on tcp/unix/noise — silently, because a dropped future is not an `Err` | **measured** |
| **H6** | admission is unauditable | no line names the peer, its transport or its origin; a session's end is `debug` | read |

## §2 The RCA: hypotheses, with their falsifiers

The recorded defect was *"the websocket fetch does not complete"* (`endo-spike/run-3.txt`). The RCA's
first job was the **cheapest decisive measurement** — the lesson the C207 RCA wrote down — and it
reversed the reading.

| # | hypothesis | falsifier | verdict |
|---|---|---|---|
| **R1** | the node fails to answer a delivery on the websocket transport | instrument `WsConn::recv`/`send` and look for a delivery that arrives | **refuted.** 54 B in (challenge), 176 B out (envelope), 333 B in, 420 B out, then **silence**. No delivery ever arrived. |
| **R2** | the peer sent a delivery and we dropped it | the same trace would show bytes after the handshake | **refuted by R1's measurement.** |
| **R3** | the connection was reset by the peer | a reset after `session established` with no close frame | **refuted.** The reset is `run-ws.sh`'s own 120 s `timeout` killing the client. |
| **R4** | the peer stalls in its own client after `session established` | the same client over `tcp-testing-only` reaches `applyMethod … fetch` one line after `session established`; over websocket it never does | **confirmed by comparison**, not by inspection of Endo's code. |
| **R5** | the node's session loop died silently, hiding the answer | the loop's `Result` was discarded; reading it would show an error | **confirmed as a defect, not as the cause** — the loop ended only when the harness killed the client. **Fixed here** (logged at `debug`). |
| **R6** | our responder path is verified against the reference | the harness's own doc asserts a reverse run | **refuted** — `run.sh` invokes the harness once (row F1). |

**The mechanism, in one sentence.** The node answers the challenge, completes `op:start-session` in
both directions, and waits; the peer never reaches its own send — so the unanswered fetch is **the peer
not asking**, and the falsifier that would settle whether it is ours is named and not run: have the
client send a `fetch` explicitly on the established session, or swap roles so this repository's client
dials Endo's websocket server.

## §3 Proposed remedies

**Tier 1 — done in this study** (each with the test that would have caught it): the short-frame panic;
the Noise send bound and the receive-bound unit; the websocket non-binary-frame handling; the
websocket handshake size bound and the post-buffer check; the challenge arity/length; the identity
file's create-mode, read-mode, all-zero and exclusive-creation checks; the session loop's discarded
`Result`; the websocket dial-policy blindness.

**Tier 1, closed after the study — the must-fix pair (D1 and D6), which the register had left open.**
The verified key the handshake produced is no longer discarded: `NetConn::verified_peer` keeps it,
`Session::verified_peer_key` exposes it, and `owner::peer_key` reads the locator's `verify` hint —
the field a transport *checks* — in preference to the peer's asserted designator, with
`accept_and_book` writing the *proved* key into that hint on an accepted session. The peer's own
hints are left alone, because they are where it is dialled back, and the locator the session was
booked under is returned so `forget` takes the same key. Both rows are **fixed here for the transport
that can prove a name** — over `websocket` the accepted side has nothing to prove (D2), so the
assertion is all there is, and that is the transport's protocol rather than a gap this code leaves.

**Tier 2 — registered, not done**: `bind_tls` being unreachable (E13), and the dialed-session count
not drawn from the session ceiling (C241).

**Tier 3 — corrected in the docs and comments**: five stale claims (F1, F3, F4, F6, F8) and the
`listen_tcp` designator comment.

**Not proposed, deliberately.** No steady-state read timeout on an established session: the prior study
declined it for the same reason it holds now — three suite tests need a silent leg — so B3/C2's
*lifetime* half is an accepted residue whose *consequence* this study states, not a defect to fix by
breaking the suite. The other half — that the refusal was transport-blind — is a defect and was fixed:
each transport holds its own share of the ceiling (see Table A row B3/C2, and §69).

## §4 The worksheet

**Staffing**: six lenses, a red team that ran its attacks against a live node, a steelman, two
adjudicators. **Severity**: S1 chain-visible/node-down · S2 node-level resource or authority,
recoverable · S3 one session/task · S4 diagnostic. **Likelihood**: L-A any peer that reaches the port ·
L-B a peer speaking the protocol · L-C operator misconfiguration · L-D a race or a fixture shape.

### Table A — deviations

Rows carry the **adjudicated** disposition (see §6 where the steelman and the adjudicator disagreed).
`fixed here` means it landed in this study's commits on `ocapn/noise`.

| # | node | word | deviation | known | S | L | steelman | disposition |
|---|---|---|---|---|---|---|---|---|
| **A1** | N1 | Less | a framed body's final chunk of 1..15 bytes reached `decrypt_in_place`, which asserts `>= 16` — **a peer that completed the handshake panicked the session task** | measured | S3 | L-B | fails | **fixed here** (both the body and every chunk are checked before the call; `a_body_shorter_than_a_tag_is_refused_rather_than_panicking`) |
| **A2** | N5 | More | `NoiseConn::send` had no plaintext bound — it would emit up to 4 GiB, which its own receive side refuses | reasoned | S3 | L-D | fails | **fixed here** (`MAX_MESSAGE_BYTES`) |
| **A3** | N5 | Reverse | the receive bound was `MAX_MESSAGE_BYTES + TAG_LEN` — the wrong unit, since a 4 MiB plaintext is 65 cipher messages | reasoned | S3 | L-D | fails | **fixed here** (`MAX_CIPHERTEXT_BYTES`) |
| **A4** | N2 | As well as | `into_data` returns text-frame payloads and **close-frame reason bytes**; `recv` special-cased close only, so a peer's close reason reached the CapTP dispatch — and the handshake read would *sign* whatever a peer put in a close | read | S3 | L-B | fails | **fixed here** (binary only; ping/pong skipped; close is end-of-stream) |
| **A5** | N5 | None | the handshake read had **no size bound**, on the path that signs what it reads (`H2`) | measured | S2 | L-B | fails | **fixed here** (`ws_config`) |
| **A6** | N5 | Late | the 4 MiB check ran *after* `into_data`, so tungstenite's 64 MiB default was the operative allocation bound | measured | S2 | L-B | fails | **fixed here** (`ws_config`) |
| **A7** | N2 | Less | the responder signed an `init:peer-auth` of any length and did not pin the record's arity | read | S4→**S2** | L-B | fails | **fixed here** (arity 2, payload exactly `CHALLENGE_LEN`) — **S2 and not S4**: with A5 it was the signing half of `H2` |
| **A8** | N1 | Other than | the Noise chunk boundary is private and unnegotiated; multi-chunk interop is unverified | read | S3 | L-B | **saves** (no counterpart exists to negotiate with) | **registered** |
| **A9** | N1 | Less | a 1..3-byte truncated length header is reported as a clean end of stream — diverging from `framed`'s own rule | read | S4 | L-D | **partly** (cannot lose a message; breaks the crate's stated discipline) | **fixed here** — the header is read by a loop that separates a close *at* a boundary from a close inside one, as `framed` does |
| **A10** | N7 | More | `Value::Int` is arbitrary precision | reasoned | S4 | L-B | **partly** (the resource is capped at 4 MiB; a 4 MiB int is *smaller* than its input) | **refuted** |
| **A11** | N1 | Late | the responder writes its SYNACK before verifying the initiator's payload | read | S4 | L-B | **saves** (XX binds the initiator's static only in message 3 — the order is forced) | **refuted** |
| **B1/C1/E3** | N2/N4 | As well as | the ws accept runs TLS+upgrade+challenge inside the accept loop, holding it up to 40 s | read | S2 | L-A | **partly** — the loop-hold is **measured false**; a ws-**only** node still holds one establish | **registered** (the doc's residual claim corrected; the ws-only case stands) |
| **B2** | N4 | Other than | the non-`biased` `select!` drops a pending ws accept when another arm fires, **silently resetting an honest peer mid-challenge** (`H5`) | measured | S3 | L-D | **partly** (self-healing; a `biased` select would be worse) | **fixed here** — one accept task per transport, and the falsifier parks a websocket upgrade while a connection lands on `tcp` |
| **B3/C2, C3/E4** | N7/N4 | No | no idle or lifetime bound on an established session: 64 post-handshake silent peers hold every permit, and the refusal is transport-blind (`H3`) | measured | S2 | L-A | **saves** (deliberate; the prior study declined a steady-state timeout) | **fixed here, for the half that is a defect** — each transport now holds its own share of the ceiling and the shares sum to it, so the unauthenticated path cannot take the whole surface; the *lifetime* half stays a decision, now stated as one rather than left implicit |
| **B4** | N7 | Other than | the enlivener and greeter await a dial-and-fetch inside `handle_deliver`, on the session loop | read | S3 | L-B | **saves** (fixtures are per-session: a peer stalls only itself) | **closed by decision** — inconsistent with Law 61, blast radius one self-inflicted session |
| **B5** | N6 | As well as | `DialPolicy` calls a **blocking** `to_socket_addrs` on the async dial path | read | S3 | L-B | **partly** (bounded; only for a name target) | **fixed here** — the dial path judges a name through `permits_from_async`, which resolves it on a blocking thread; literals are judged inline as before |
| **B6/C8** | N6/N7 | No | dialed sessions are not counted against `MAX_SESSIONS` | read | S3 | L-B | **partly** (bounded by `MAX_PEERS = 256`, not unbounded) | **registered** |
| **B7** | N7 | More | each `Reply::Deferred` spawns a waiter task polling the global gift store 10 ms for 10 s; the cap bounds landed answers, not waiters | read | S2 | L-B | **partly** (each is O(1) and short-lived; the "absent gift" behaviour is correct) | **fixed here** — live waiters are capped per session (`MAX_DEFERRED_WAITERS`) and the delivery past the cap is refused with a `break` |
| **B8/E15** | N8 | Reverse | shutdown drains the listener but not its detached session tasks | read | S4 | L-B | **partly** (the deploy is submitted before the reply wait; only the peer's answer is cut) | **fixed here** — the operator's stop word ends a session too, so a drain drains the sessions it started |
| **B9** | N4 | Late | the ceiling is consulted after establishment | read | S4 | L-B | **saves** (the conn does not exist before that; checking earlier would block *all* accepts) | **refuted** |
| **C7/E14** | N4/N8 | More | one WARN per refused accept | read | S4 | L-B | **partly** (bounded at ~10/s by the backoff) | **closed by decision** — it is the operator's signal |
| **C9** | N7 | More | one node-global 4/s deploy limiter; the fixed window admits ~2× across a boundary | read | S2 | L-B | **saves** (fairness is declined with Law 63a) | **fixed here** — `RateLimiter` is a token bucket, so a client that keeps asking is held to the rate rather than re-granted a whole allowance at each boundary |
| **C10** | N7 | More | bridged deliveries poll the chain API ~480×/s node-wide | reasoned | S4 | L-B | **saves** (a product of two shipped bounds) | **refuted** |
| **C11** | N1 | More | `open` reassembles with `drain(..take)` per chunk — O(n²) | reasoned | S3 | L-B | **partly** (bounded to ~128 MiB of copy at the cap; no amplification) | **fixed here** — the body is walked by offset rather than drained; the complexity change itself is reasoned, not measured |
| **C12/D3/E5/E6/F7** | N3 | Late/Early | the identity file's mode was set **after** the write and **never checked on read** | read | S2 | L-C | fails | **fixed here** (`create_new` + `mode(0o600)`; the read refuses a loosened file) |
| **D1** | N1 | Other than | the transport-verified Ed25519 key is discarded (`let _…`) and never bound to the peer identity | read | S3 | L-B | **saves** (no binding is defined by the spec or the reference; it is Law 63a's work) | **fixed here** — `NetConn::verified_peer` keeps the handshake's proved key, `Session::verified_peer_key` exposes it, `peer_key` reads it in preference to the asserted designator, and `forget` takes the key the session was booked under |
| **D2** | N2 | No | the websocket handshake authenticates only the server | measured | S2 | L-A | **saves** (the reference's own shape, documented, off by default) | **closed by decision** |
| **D4** | N3 | As well as | the identity file *is* the name, so two processes sharing it are one peer; no rotation | read | S2 | L-C | **partly** (operator error; rotation would rename the node) | **fixed here** — stated in the page, `defaults.conf` and the config field: one file is one peer, and the name cannot be rotated without it moving |
| **D5** | N3 | Less | a 64-byte all-zero file passed as a valid identity | read | S2 | L-C | fails | **fixed here** |
| **D6** | N1/N2/N6 | Other than | the registry keys on the peer's **self-asserted** designator, so a peer knowing an honest peer's public name can, by the crossing rule, evict that peer's accepted session | read | S3 | L-B | fails | **fixed here, for the transport that can prove a name** — the registry is keyed by the proved key where there is one, so over `noise` a peer naming itself another's name is filed under its own and cannot collide; **over `websocket` it remains, by the protocol rather than by this code** (D2: only the server proves itself) |
| **D7** | N4/N8 | More | the chain capabilities are published to every admitted session regardless of transport | read | S2 | L-A | **saves, reframed** (no transport grants differential authority — it restates A2/D1) | **refuted** as a distinct row |
| **D8/F10** | N3/N8 | Other than | the name is a function of configuration, not of the node; the `listen_tcp` comment still attributes it to `node_designator` | read | S3 | L-C | **partly** (the behaviour is required — the name must be the key the handshake checks) | **fixed here** (the stale comment; the consequence is documented) |
| **D9/E1** | N8 | No | admission is unauditable; a session's end is `debug` | read | S4 | L-A | **partly** (some visibility exists; the omission is deliberate, against log flood) | **fixed here** — both lines are `info`, naming the peer and the transport, through a rate limiter rather than either silent or a flood |
| **E7** | N3 | No | a truncated identity file bricks startup with no regeneration path | read | S2 | L-D | **saves** (regenerating would rename the node and orphan peers) | **closed by decision** |
| **E9** | N8 | Other than | `ocapn-listen-noise = 0.0.0.0:<port>` advertises host `0.0.0.0` — a location a *remote* peer cannot dial back | read | S3 | L-C | **fails unless the peer is on the same host** | **fixed here** — `api-server.ocapn-advertised-host` names the host peers are told, and an unspecified bind without one is refused at startup; the design page's own example, which used `0.0.0.0`, is corrected |
| **E10** | N6 | No | the dial policy never judged a websocket target (it reads `host`; a ws locator carries `url`) — **a measured SSRF** (`H4`) | measured | S2 | L-B | fails | **fixed here** (`host_of` parses the authority, including bracketed IPv6; `a_websocket_target_is_judged_by_its_url`) |
| **E11** | N6 | Less | `DialPolicy::allow` is unreachable from configuration | read | S4 | L-C | **saves** (harden-by-default; the seam exists for tests) | **closed by decision** |
| **E12** | N8 | No | `ocapn-identity-key` with no listener is silently ignored | read | S4 | L-C | **partly** (nothing consumes it) | **fixed here** — read and validated whenever it is set, and still not adopted; a malformed key is now a startup error |
| **E13** | N8 | No | `bind_tls` has no caller and no config key; the module doc advertises `wss://` | read | S4 | L-C | **partly** (the node never advertises `wss://`, so nothing advertised is unreachable — the *doc* overstates) | **fixed here** (the doc) |
| **E16** | N8 | No | the dial route's request doc names only the pre-existing transports | read | S4 | L-C | **saves** as an enumeration; the example is stale | **fixed here** (the doc) |
| **E17/F1** | N8 | No | the noise harness's source doc asserts a reverse-direction run that `run.sh` does not make | measured | S3 | L-C | fails | **fixed here** (the comment) |
| **F3/F4** | N8 | Less | the node page's netlayer table lists two transports and says "the only one of the three" | measured | S4 | L-C | fails | **fixed here** |
| **F6** | N8 | Early | `run-ws.sh` says the identity file appears "as it binds"; it is written during setup, before the bind | read | S4 | L-D | fails | **fixed here** (the comment) |
| **F8/F9** | N2 | Less | the websocket test is named for "the transport with a live peer" but dials with our own client | read | S3 | L-C | fails | **fixed here** (the doc) |

**Vacuous words, with their reasons** (recorded because a bare "n/a" is a defect): `Early` is vacuous at
N6 (the target policy decides an address before a connection and bounds no timed resource); `Reverse` is
vacuous across the transports (a message has a fixed direction set by the netlayer, so there is no
quantity to invert — the two asymmetries the lens found are an absence, C5, and a timing, C4); `As well
as` is vacuous at N1/N2/N3/N5/N6/N8 and a row only at N7 (the dialed session beside the accepted one).

### Table B — barriers (attacks the red team ran that **failed**)

| attack | defence | measured |
|---|---|---|
| a websocket frame declaring 100 MiB / 1 TiB / `2^63-1` | tungstenite's frame cap refuses on the 10-byte header | all three closed in 0.00 s; log `Space limit exceeded` |
| a 17 MiB single frame | the same cap | `ConnectionResetError` after 1.84 s |
| a first frame that is not `init:peer-auth` | `is_peer_auth` | refused in 0.00 s, no node write |
| 8 MiB of random bytes as the first frame | `Value::from_bytes` | refused; `syrup: … trailing bytes` |
| a Noise SYN whose 32-byte prefix names another responder | the prefix check before any cryptography | refused in 0.00 s |
| a Noise initiator signing with a key other than the one it names | `check_payload` | node closed; no session |
| a Noise frame declaring 4 MiB + 17 | `recv`'s bound | closed, no panic |
| 300 ws auth cycles + 300 noise cycles + a 64-connection flood, then release | one task and one permit per session, RAII-released | RSS 97396→97376 kB, fds 42→43 across 600 cycles; **RECOVERED** afterwards |
| a silent peer on either transport | `HANDSHAKE_TIMEOUT` / `AUTH_TIMEOUT` | closed at 30.00 s on both |

**And one barrier that is not one.** The red team's first entry was *reported* as a failed attack — a
silent websocket peer "cannot hold the loop" — and that measurement is correct, but it **refutes this
repository's own comment** rather than demonstrating a barrier: `tokio::select!` drops the pending
accept future when another arm completes. The same mechanism is the defect in row **B2**. A "barrier"
that protects loop occupancy by trading away a connection's availability is not independent of the
threat, and it is recorded here as such rather than counted as a defence.

## §5 The merged fault tree

**Top event** — *an unauthenticated remote peer holds all 64 OCapN session permits, so every honest
connection is refused on every transport* (S2, **measured**: 64 bare TCP connections, zero bytes each).

```
TOP  one transport's whole share held by one unauthenticated peer; honest sessions on *that*
     transport refused — the node's other transports keep their own shares.
     [api/ocapn.rs Semaphore::new(share), try_acquire_owned, warn+drop, one per transport]

  OR
  ├─ A1  hold an ESTABLISHED session and go silent.        [B3/C2]  ← reaches TOP, NO barrier
  │      the permit is bound to the session task and released only when it ends; the loop awaits
  │      `conn.recv()` with no timeout. The handshake bound stops strictly BEFORE this line.
  │      Permanent, not renewable — this is what makes the denial sticky. **Bounded to one
  │      transport's share and left as a decision**: an idle session is legitimate, and
  │      `HANDSHAKE_TIMEOUT`'s note records why the steady-state read is not bounded.
  │
  ├─ A2  hold a share of UNESTABLISHED sessions and renew.  [C3/E4]  ← barrier PARTIAL
  │      HANDSHAKE_TIMEOUT 30 s / AUTH_TIMEOUT 30 s are real anchors, but the permit is taken
  │      BEFORE the handshake and the bound is per-peer: a share's worth of silent sockets takes
  │      the share for 30 s, renewable at that many sockets per cycle. The per-peer bound does not
  │      compose into a node bound, and what makes it a *bounded* problem rather than the whole
  │      surface is the share.
  │
  └─ A3  churn connections faster than tasks end.        [red team]  ← SEVERED
         RAII permit release + ACCEPT_BACKOFF. Independent of the threat; measured holding.

  AMPLIFIER, now severed: the ceiling was ONE transport-blind semaphore, so the unauthenticated
  `websocket` path could starve the authenticated `noise` path. **Fixed here** — every transport
  holds its own share of the ceiling, and the shares sum to it rather than nesting under it, so the
  amplifier is gone and the top event is one transport's share rather than the node's surface.

  NOT BARRIERS, named: the share is the bound whose exhaustion IS the top event (of that transport);
  the ceiling warn+drop observes the fault rather than preventing it, and names no peer — it now
  names the transport, which is what tells an operator *where* to look.

  SIBLING TOP EVENTS, named and not folded (each is a different asset):
   · H1 the noise short-frame task-kill            [fixed here]
   · H4 the websocket dial-policy SSRF              [fixed here]  — authority, no permit
   · H5 the cross-transport ws cancel               [fixed here]  — one connection
   · D6 a peer evicting an honest peer's session     [fixed here]  — a different peer's session
                                                     (over `noise`; unchanged over `websocket`)
```

## §6 The RCA, adjudicated — including where the two adjudicators disagreed

The steelman and the adjudicator disagreed on **eight row groups**. That disagreement is the study's
most useful output, because each side is right about a different thing; the verdicts:

| rows | adjudicator | steelman | **adjudicated** | why |
|---|---|---|---|---|
| **D1** | must fix | saves (no binding is defined by the spec or the reference) | **fixed here** | The steelman was right that no *protocol* binding exists to violate, and the adjudicator was right that the material is now in hand: *keeping* the key is a patch, not a binding, and D1 as stated — the key is discarded — is what the patch removes. It is Law 63a's work and the input D6 needs. |
| **D6** | must fix | **fails** (a stranger who knows the public name can force an eviction) | **fixed here** over `noise`, **unchanged over `websocket`** | Both agreed it is real; the steelman's own verdict was `fails`. Keying by the proved name *is* the design change that was called for, and it is bounded to the transport that proves one: a websocket accepted session has no proved name to key on (D2), so there the assertion remains all there is — a weakness of that transport's protocol, recorded as such rather than left as a hole here. The key went into the `verify` hint rather than over the designator, so a peer that has an identity keeps exactly the name it had. |
| **B2** | must fix | partly (self-healing; `biased` would be worse) | **fixed here** | The steelman's objection was to the *proposed* fix, not the defect, and it stands: `biased` would let one silent arm starve the others. Per-transport accept tasks remove the cancellation without it, which is what landed. |
| **B4** | should fix | **saves** (per-session blast radius) | **closed by decision** | The steelman is right: the fixtures are rebuilt per session, so a peer stalls only itself. It stays inconsistent with Law 61 and is recorded as such. |
| **B9** | should fix | **saves** (checking earlier would block all accepts) | **refuted** | The steelman's ordering argument is decisive; the permit cannot precede the connection's existence. |
| **C9** | register | **saves** (fairness is declined with Law 63a) | **fixed here** | Only the fixed window's 2× boundary was a defect, and it is one now closed in the limiter itself; the global-vs-per-peer half is the declined decision and stays declined. |
| **D7** | should fix | **saves, reframed** (no transport grants differential authority) | **refuted** as a distinct row | It restates A2/D1: closing websocket would change nothing, because none of the transports binds a deployer identity. |
| **E11, E16** | register | **saves** | **E11 closed by decision; E16 fixed here** | E11 is harden-by-default with a test-only seam. E16's JSON is an example rather than an enumeration — but the example *is* stale, so the doc is corrected. |

**And the steelman found what the adjudicator missed**: `E9`'s `0.0.0.0` trap was not only in the code
but in the **design page's own example config** — a remote peer dialling that back-address reaches
itself. Both halves are fixed: `api-server.ocapn-advertised-host` names the host peers are told, an
unspecified bind without one is refused at startup, and the example now carries the key.

**What would have caught all of this earlier, cheaply.** Two things, both of which now exist:
instrumenting the websocket connection (the RCA's trace), and **running a foreign peer** — the
`desc:sig-envelope` gcrypt defect, the SSRF, and the ceiling lockout were all reachable only that way.
The unit tests agreed with themselves throughout.

## §7 The verdict

**The transports are sound in their cryptography and were unsound in their edges.** The Noise handshake
is verified against the implementation it was modelled on and its bounds are now correct in both
directions; the websocket challenge is bound to a length and a record shape; the identity file is
created exclusively and checked on read. What the study found is that the edges — a body length, a
frame type, a size check's *ordering*, a policy that read one hint and not another — are where a
transport's defects live, and that **the same class recurred**: a check placed after the work it
guards (A6, B9), a bound expressed in the wrong unit (A3), a resolver the policy could not see (E10).

**One row is the study's own indictment.** `ocapn/src/websocket.rs` carried a comment stating a
residual — "a peer that connects and then stays silent holds that loop" — that measurement shows is
false in a multi-transport node, while the *real* hazard in the same mechanism (B2, an honest peer
silently reset by unrelated traffic) went unnamed. **A stated residual that is wrong is worse than no
residual**, because it ends the search.

**The process verdict.** Ten agents; two were blocked and re-run; the re-run produced eight
disagreements and one measurement that refuted the code's own comment. Had the study shipped the first
pass — or had it skipped the steelman because the adjudicator had already ruled — it would have
recorded eight rows as settled that nobody had argued against.

**The disposition, taken rather than implied: both transports ship, and the weaker one ships with its
weakness recorded.** `websocket`'s accepted-side defect — no mutual authentication, so an accepted
session's name is the peer's assertion and nothing more (D2 → C243) — is *the reference's protocol*,
not this implementation's, and withdrawing the transport would withdraw the only one a **published**
Agoric peer (`@endo/ocapn` 1.1.1) speaks. What makes shipping it defensible is that it is not on by
default, that the node page and this study both say plainly what it does not provide, and that the
stronger transport exists for a deployment that wants the property: the alternative to a documented
weak transport is not a strong one, it is a peer that cannot connect at all. `noise` ships without
reservation — its cryptography is verified against the implementation it was modelled on and its
falsifiers are the interop run and the tests this pass added. So the row's own alternative —
withdraw — is declined with that reason, and C243's residual on the `websocket` row is the record of
what was declined rather than a hole nobody noticed.
