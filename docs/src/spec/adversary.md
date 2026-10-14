# The bounded adversary this testnet claims to handle

Criterion 3 of [the acceptance specification](testnet-acceptance.md) is not a pass/fail item. It asks for
**a rate and a class census against a bounded-adversary statement** — and the census cannot be run until
the statement exists. This is the statement. Every row is meant to be falsifiable, and the falsifiers are
in the table rather than implied.

## The model

| | |
|---|---|
| **Validators** | four, equal stake of 250, pool 1000. The assumption is **at most one Byzantine**, i.e. **less than a third of the stake** — with unequal stakes the bound is by stake, not by count |
| **Network** | partially synchronous. An adversary may delay, drop, reorder or **selectively withhold** messages, for an unbounded period |
| **The operator** | a trusted operator exists who can run `tools/reconcile-network.sh` against a stalled net, and may name the reconciliation anchor (`RECONCILE_ANCHOR`) |
| **Time** | no bound on recovery, except where a rule provides one — several do, and each is named below |

Two things are **explicitly outside** this statement, because saying so is more useful than implying
otherwise:

- **Whether an already-finalised write can be undone by a later merge.** That is a *safety* question, the
  acceptance page deliberately excludes it ("a liveness and membership verdict, not a safety verdict"),
  and it is **open**. Nothing below should be read as covering it.
- **Post-quantum adversaries.** Tracked separately.

## What "handled" means here

For this statement, a class is handled when a bounded adversary cannot (a) make two honest nodes accept
conflicting finalised state, or (b) stop the net indefinitely once the validators have returned. Class (a)
is *assumed* and not proved by this page; class (b) has been measured, and the measurements are cited.

## The census

| Class | What bounds it | What does not | Falsifier |
|---|---|---|---|
| **Equivocation** — one validator signing two blocks at a height | Detectable from the chain alone: every attestation is a signed block, so two blocks from one sender with the same justification are a proof anyone can hold. Measured clean on the incident capture (`staleSnapshotSelfEquivocations=0` on all four) | **Its cost: there is no `slash`.** Misbehaviour is detectable and free ([#290](https://github.com/rchain-community/rchain-rust/issues/290)) | Two signed blocks, one sender, one height, one justification |
| **Withholding and selective delay** | The merge relation is on the chain and per-deploy provenance is preserved, and the fringe-key store that could hold two states for one key has been **removed** — the divergence class is now unrepresentable, by construction rather than by tolerance | **Liveness under partition.** Withholding was measured to *induce* divergence between honest nodes, with no fault present, which is why this row names the mechanism rather than the actor | Two honest nodes computing different states from the same history |
| **Silence of a quorum** — two of four stopped | The strict rule stops finality correctly at 50 % (measured: frozen at 323 while height ran to 334, `500 of 1000 (2 full partitions)`) | **Nothing bounds when they return** — this was the defect: a sub-quorum outage did not resume even after a full restart ([#315](https://github.com/rchain-community/rchain-rust/issues/315), closed as refuted on the current build, residue in [#328](https://github.com/rchain-community/rchain-rust/issues/328)) | Stop two of four, restore both: does it resume with no operator action? |
| **Deploy flooding** | `MAX_POOLED_DEPLOYS = 10_000` (a soft bound), `MAX_BLOCK_DEPLOYS = 255`, and a phlo pre-charge that requires the deployer to hold REV | **The rate from one identity.** The faucet's limits are per *address* (ten drips, 0.3 REV each), and addresses are free | The pool's bound and the pre-charge, or the node's own memory |
| **State divergence at a fringe key** | Removed, not tolerated: the store that held a derived value at a fringe key is gone, and every exit of the merge walk is checked | — | One fringe recorded with two states |
| **A validator rejecting its own block** | Fixed: the node's view of the parents can no longer differ from the proposer's in the way that produced it ([#316](https://github.com/rchain-community/rchain-rust/pull/316), merged) | — | A node's own block failing its own validation |
| **Operator error during recovery** | The tool refuses rather than choosing: it stops when the nodes' finalised blocks disagree and it cannot derive the point, and it preserves the genesis files across a wipe | The operator's **choice** of anchor is a trusted action — by design, and named here so it is not mistaken for a derivation | A reconciliation that selects a winner by preference rather than by the rule |
| **Key compromise** | The stake bound: one compromised validator is the assumption | — | A signed block from a compromised key that the others accept |
| **Post-quantum** | Nothing | Out of scope; tracked in [#274](https://github.com/rchain-community/rchain-rust/issues/274) | — |

## How to use this page

The census is run **against this statement**, and a row passes only with a committed run and its
falsifier armed — the same rule the acceptance page applies to criteria 1 and 2. Where a cell says a
class is *assumed*, that is exactly what it is: not measured, and not to be reported as verified.

Two rows are worth watching as the net matures, because they are the ones where "handled" currently
depends on something outside the protocol: **equivocation has no cost**, and **recovery depends on an
operator** who is trusted to choose an anchor. Neither is a defect; both are bounds, and a reader deciding
what to trust should know they are there.

## The wargame — production readiness, from an apriori standpoint (2026-10-13)

The census above is the adversary statement this network *claims* to handle. This section tests that
claim by a different method: not a checklist and not the register, but a **tabletop wargame** over the
node's **production surface**, run **apriori** — from the code, with the register bracketed.

**Method.** A HAZOP leader drives the eleven IEC 61882 guide words against each study node; **personas**
(Alice, Bob, Eve, Mallory, Oscar, Root, the Operator) act each deviation out; and **every candidate
finding is handed to a separate referee** that re-derives it from the source and returns one verdict —
`CONFIRMED` (with the artifact), `NOT-RE-ESTABLISHED`, or `REFUTED` (with a positive artifact; an
empty-handed refutation is not permitted). The referee is told the finding and the code, and is **not**
told the proposer's reasoning, the persona, the other findings, or whether a register row exists. The
join to the register is a **separate stage**.

**What the run was.** 140 agents on `dev` at `1419b127d`. Three *pre-registration* decisions make it a
test rather than a worksheet: a persona is a **capability** (a deviation with no cited reachability fact
is `UNSTATABLE`, not a row); a guide word with no bounded property is **vacuous with a reason** (vacuity
47%); and the referee's calibration is measured by **seeded controls** mixed into its queue. **The
instrument was falsified twice before it was trusted** — the first pilots leaked the register into the
referee and over-confirmed a false control — and the run below is the third, with calibration **6 of 6**.

**The study nodes** are the production functions, not the register's U1–U16 — a transport, a deploy
ingress, the public reads, the admin actuator, the key lifecycle, the host, the build, the capability
wire, the validator, observability, upgrade/DR, cost, entropy and time. Each is bound to the census row
**it tests**, which is what makes the section a test of the statement rather than a page beside it.

**Table A — deviations.** Every row is a persona acting a guide word against a node, ruled on by the
referee. **`Precondition` is the context that makes the row a *finding*** — the actor-condition and
surface under which it is an exploit, a vulnerability, or a failure mode. A row is a finding only if that
context holds; the vocabulary separates **`remote · default config`** (an exploit on an unconfigured
node) from `misconfig`, `dev-mode`, `build-time`, `same-host`, `opt-in` and `no adversary` (a failure
mode) — so a conditional row cannot read as an unconditional one. `Disposition` is `—` for an attack
finding (the verdict is the outcome) and a rung R1–R4 for an operator **fault**. Every row is
`read`-class evidence: the falsifier is `owed` unless a committed run arms it.


## Table A — deviations by node (with Table B barriers)
### W-N1 — peer transport + discovery

**Design intent.** a peer's message is authenticated and bounded in size and rate.
**Census row tested.** equivocation, withholding
**Guide words.** row: AFTER, AS WELL AS, EARLY, LATE, LESS, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: BEFORE (On the unary path the network-id and sen…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N1-01** | Eve | MORE | The Kademlia discovery server is served with NO inbound decode cap, so it accepts messages up to tonic's default (4 MiB) — 16x the operator's configured protocol-server.grpc-max-recv-message-size (256… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/discovery/grpc_kademlia_rpc_server.rs::an_oversized_lookup_never_reaches_the_handler` — **armed 2026-10-14**: the decode cap holds: a 1 MiB `Lookup` sent to a server booted with the operator's 256 KiB cap never reaches the handler, where deleting `.max_decoding_message_size(..)`… (tonic's 4 MiB default) lets it through. |
| **W-N1-02** | Eve | PART OF | Authentication covers only the PART of a peer message that carries a well-formed header. On the unary path the wrong-network rejection is inside `if let Some(header)` (line 386) and the sender-identit… | authority-widening | inside | remote peer · any valid cert (no allow-list) | REFUTED | — | `comm/src/transport/grpc_transport_receiver.rs::a_sender_must_be_the_identity_its_certificate_proves` — **armed 2026-10-14**: a Protocol whose sender its certificate does not prove — and one with no proven identity at all — is refused `PermissionDenied` before the dispatcher sees it. |
| **W-N1-03** | Eve | REVERSE | A peer's LookupResponse is acted on in the REVERSE direction — the node dials a target the peer named. The discovery client collapses the response's nodes into peers with NO local-address filter (`.no… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/discovery/grpc_kademlia_rpc.rs::a_lookup_response_naming_a_local_host_is_not_dialled` — **armed 2026-10-14**: a `LookupResponse` naming a loopback host is not recorded or dialled. |
| **W-N1-04** | Mallory | NO | There is no rate bound on the inbound transport surface; 'rate' is realised only as in-flight CONCURRENCY, and the permit is released the instant a dispatch/stream completes, so an anonymous peer's su… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `spec/audit/passes.md::W-N1-04` — **armed 2026-10-14**: decided, not armed: there is no *rate* bound on the inbound transport surface and there is not meant to be — what bounds it is in-flight **concurrency**, keyed on nothing because it bounds a resource rather than a party (§116, register C290); `a_full_dispatch_queue_is_refused` is the bound that exists. |
| **W-N1-05** | Mallory | LESS | The Kademlia rate bound is one GLOBAL token bucket shared by every peer, so one anonymous peer spending it reduces every other peer's allowance to zero — the rate a victim actually receives is far les… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/discovery/grpc_kademlia_rpc_server.rs::refused_kademlia_requests_do_not_spend_the_allowance` — **armed 2026-10-14**: the allowance is spent on admitted work: a flood of refused requests does not drain an honest peer's budget, and the limit refuses past its bound with `ResourceExhausted`. |
| **W-N1-06** | Mallory | OTHER THAN | A rate/concurrency rejection is reported to the sender as a SIZE error: ResourceExhausted (dispatch/stream/blob slot exhausted) maps to CommError::MessageTooLarge, so 'the bound refused you' is indist… | observability-gap | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | already fires — comm/src/transport/grpc_transport.rs:386-389 asserts the ResourceExhausted -> MessageTooLarge mapping after a full dispatch queue; the… |
| **W-N1-07** | Mallory | AS WELL AS | A heartbeat does MORE than signal liveness: it moves the sender to the END of the bounded connection table, so a heartbeating peer escapes the oldest-first eviction that a handshake-only peer suffers.… | authority-widening | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | owed: fill the table with MAX_CONNECTIONS peers, heartbeat one, add a new peer, and assert the heartbeated peer survives the eviction while an un-heartbeate… |
| **W-N1-08** | Mallory | EARLY | The Kademlia rate token is spent BEFORE the network-id and identity checks, so an anonymous peer can drain the shared bucket with wrong-network or spoofed-sender requests the node rejects anyway — the… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/discovery/grpc_kademlia_rpc_server.rs::refused_kademlia_requests_do_not_spend_the_allowance` — **armed 2026-10-14**: a flood of pings the guard refuses costs the honest peer nothing — the token is charged after the sender is examined, not before. |
| **W-N1-09** | Oscar | LATE | The streamed path's sender/network binding runs LATE — after the whole stream has been received and metered. The circuit breaker that compares the stream header's sender to the certificate-proven id, … | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/transport/grpc_transport_receiver.rs::a_stream_header_is_admitted_only_for_the_identity_its_certificate_proves` — **armed 2026-10-14**: the streamed header's admission decision runs when the header arrives rather than after the drain loop has buffered, size-checked and budget-charged every chunk (the test names both rows). |
| **W-N1-10** | Alice | AFTER | On the streamed path the message's bytes are read and buffered, and a stream_slots permit is taken, BEFORE the sender/network identity is checked — the WrongNetworkId / SenderNotVerified circuit break… | resource-amplification | inside | remote peer · any valid cert (no allow-list) | CONFIRMED | — | `comm/src/transport/grpc_transport_receiver.rs::a_stream_header_is_admitted_only_for_the_identity_its_certificate_proves` — **armed 2026-10-14**: the same decision, which is the breaker's own rule, so a header that could never verify is refused before it buys a stream. |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Eve | Barrier: every surface of N1 is mutual-TLS and every message is additionally bound sender↔certificate, and none of Eve's reachable plaintext ports feeds N1. The peer transport binds 0.0.0.0:40400 and the discovery server binds 0.0.0.0:40404, both served from h… |

### W-N2 — deploy ingress

**Design intent.** a deploy is admitted once; admitting is bounded per unit time per reachable identity.
**Census row tested.** deploy flooding
**Guide words.** row: AFTER, AS WELL AS, BEFORE, EARLY, LATE, LESS, MORE, NO, OTHER THAN, PART OF · vacuous: REVERSE (No mechanism reverses the admission pred…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N2-01** | Eve | OTHER THAN | The per-unit-time bound is not per reachable identity: each ingress consults a single keyless token bucket shared by the whole node, so one identity's burst exhausts the allowance and refuses every ot… | resource-amplification | inside | remote · default config | CONFIRMED | — | `tools/wargame-arms.sh::arm_deploy_admission` — **armed 2026-10-14**, two runs agreeing: the bound has a subject: a 200-deploy burst drew 88 × 429 for the flooding identity, and the same identity was then admitted over gRPC. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N2-02** | Eve | AS WELL AS | A deploy is admitted through additional independent ingresses, each holding its own fresh allowance over the same pool, so the node's aggregate bound is the sum of the per-surface limits (~2x DEFAULT_… | resource-amplification | inside | remote · default config | CONFIRMED | — | `tools/wargame-arms.sh::arm_deploy_admission` — **armed 2026-10-14**, two runs agreeing: the surfaces are not one pool: a saturating HTTP flood refused the flooding identity and admitted two gRPC deploys. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N2-03** | Eve | MORE | The pool's capacity bound over-admits: the 'pool is full' check and the write are two separate store-lock acquisitions, so concurrent deploys all pass the check before any write lands and the pool exc… | resource-amplification | inside | remote · default config | CONFIRMED | — | `casper/src/dag.rs::add_deploy_admits_exactly_one_when_the_pool_is_one_below_the_cap` — **armed**: eight concurrent `add_deploy` against a pool one below the cap admit exactly one and never overshoot. |
| **W-N2-04** | Eve | LESS | A fresh identity gets less admission than the bound implies: the 10 000-slot cap is node-global with a global refusal, so once any one identity fills the pool the node refuses every other submitter ('… | resource-amplification | inside | remote · default config | CONFIRMED | — | `casper/src/dag.rs::one_deployer_cannot_hold_the_whole_pool` — **armed**: a deployer at its share is refused with a sentence naming the share, and a different deployer is admitted into a barely-used pool. |
| **W-N2-05** | Eve | BEFORE | The term is fully parsed and normalized before the duplicate is detected, so a repeated submission of an identical deploy re-does the parse work even though the pool stores it once — 'admitted once' h… | resource-amplification | inside | remote · default config | CONFIRMED | — | owed: submit an identical large deploy K times and measure parse/normalize time; expect ~K times a single parse. A test asserting a duplicate is rejec… |
| **W-N2-06** | Eve | LATE | The HTTP rate check fires after the request body has been decoded: the limiter is inside the handler body while axum's `Json` extractor runs first, so an over-limit flood still pays the full decode/de… | resource-amplification | inside | remote · default config | CONFIRMED | — | owed: send over-limit requests with maximum-size bodies and instrument decode allocations/CPU before the first 429; a test asserting the limiter is co… | — not armed: decode-before-429 is not observable from the response alone — it needs a CPU or a counter the runner does not have. 
| **W-N2-07** | Eve | AFTER | Pool entries outlive their bound: the only evictor runs after finalization, so when finalization stalls no deploy expires, the pool fills to its cap, and the node then refuses all deploys. | resource-amplification | inside | remote · default config | REFUTED | — | `casper/src/dag.rs::inserting_a_block_expires_the_pool_with_no_finalisation` — **armed**: the pool is expired on the insert path with nothing finalised: a block at height 51 inserted into a quiet chain drops an expired deploy, and deleting the `expire_deploys` call from `insert` reddens it. |
| **W-N2-08** | Mallory | PART OF | The admission bound is a single process-global token bucket: it is 'per unit time' but carries no per-reachable-identity dimension, so one identity consumes the whole 100/s allowance and starves every… | resource-amplification | inside | remote · default config | CONFIRMED | — | `tools/wargame-arms.sh::arm_deploy_admission` — **armed 2026-10-14**, two runs agreeing: a second identity is not denied by the first's flood: admitted over gRPC while the first was refused 88 times. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N2-09** | Mallory | EARLY | On both deploy surfaces the rate token is spent BEFORE the deploy is authenticated or even parsed, so unauthenticated garbage drains the shared admission budget: the bound fires ahead of every validit… | resource-amplification | inside | remote · default config | CONFIRMED | — | owed: POST 100/s of deploys bearing deliberately-invalid signatures from a fresh keypair; a subsequent valid deploy from another identity then receives 429 … | — not armed: an invalid-signature flood needs a signed-but-wrong deploy, which this runner cannot mint. 
| **W-N2-10** | Bob | NO | Once the single global deploy pool reaches its 10,000-slot cap, a fresh, well-formed, correctly-signed deploy from a DIFFERENT reachable identity is admitted ZERO times — the capacity bound has no per… | resource-amplification | inside | remote · default config | CONFIRMED | — | `casper/src/dag.rs::one_deployer_cannot_hold_the_whole_pool` — **armed**: the pool cap is a per-deployer share: filling it from one identity does not deny another. |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Eve | Passive, read-only barrier. Eve can read every plaintext byte of a deploy (term, deployer key, signature — the deploy gRPC server is built with no TLS: node/src/api/grpc/mod.rs:167) and observe admission timing, but admission is a WRITE of a signed deploy into … |
| Oscar | Wrong surface. The admission function is client-facing (deploy gRPC 40401 / HTTP 40403) and is fed only by inbound deploy requests. Oscar's powers — sign two blocks at a height, withhold, selectively delay, lie inside the protocol — travel the block/attestatio… |

### W-N3 — public read surface

**Design intent.** the public reads disclose only what a public reader is meant to see.
**Census row tested.** — (new surface)
**Guide words.** row: AS WELL AS, EARLY, MORE, OTHER THAN, REVERSE · vacuous: AFTER (Folded into LATE: no time-shifted/cached…), BEFORE (Folded into EARLY: the pre-commit disclo…), LATE (Structural: every public read is compute…), LESS (Structural: under-disclosure (returning …), NO (Structural: the node property is a one-s…), PART OF (Structural: a read that reveals only par…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N3-01** | Eve | MORE | The unauthenticated public status read discloses node-internal operational/divergence state: the block hash+seq this node has given up on (unrestorableBlock), a standing reset marker with its anchor h… | disclosure | inside | remote · passive read (no TLS) | CONFIRMED | — | `tools/wargame-arms.sh::arm_public_reads` — **armed 2026-10-14**, two runs agreeing: the public status carries the public key set only — no operator key appears (HTTP 200 against a three-validator devnet). The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N3-02** | Eve | AS WELL AS | Every public read is disclosed to an additional recipient: because neither public listener terminates TLS, each request and its reply travel in the clear, so a passive reader on the wire reads the rep… | plaintext-transit | inside | remote · passive read (no TLS) | CONFIRMED | — | `tools/wargame-arms.sh::arm_public_reads` — **armed 2026-10-14**, two runs agreeing: the public API is plaintext: http answers and https does not. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N3-03** | Eve | OTHER THAN | GET /api/v1/pos/delegations?delegator=key serves one delegator's portfolio — every operator staked with, the amount, accrued-but-unpaid rewards, and any staged exit — to any unauthenticated caller who… | disclosure | inside | remote · passive read (no TLS) | CONFIRMED | — | `tools/wargame-arms.sh::arm_public_reads` — **armed 2026-10-14**, two runs agreeing: the delegator-scoped read is not on the public port (404) and answers on the admin port (200). The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N3-04** | Eve | EARLY | The data-at-name reads disclose a name's data from every block in the DAG's height_map, with no finality gate — so a passive reader who holds a name reads its payloads from blocks that are not yet fin… | disclosure | inside | remote · passive read (no TLS) | CONFIRMED | — | `casper/src/api/block_api_impl.rs::only_finalised_heights_are_readable` — **armed**: reads are floored at the finalised height: no block above it is served, whatever the caller asks for. |
| **W-N3-05** | Bob | REVERSE | Two routes the public router presents as reads are the inverse of a read: POST /api/explore-deploy runs the caller-supplied Rholang term and POST /api/data-at-name runs a listen, both with the node's … | unauth-actuation | inside | remote · passive read (no TLS) | CONFIRMED | — | `tools/wargame-arms.sh::arm_public_reads` — **armed 2026-10-14**, two runs agreeing: an unsigned term executed over the unauthenticated public port (200). The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |

### W-N4 — admin actuator

**Design intent.** only the trusted operator can make the node act with its own authority.
**Census row tested.** operator error
**Guide words.** row: AS WELL AS, LATE, LESS, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: AFTER (There is no post-actuation step in which…), BEFORE (Folded into OTHER THAN. The admin router…), EARLY (Signing is a precondition of the block t…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N4-01** | Mallory | NO | The admin actuator carries no caller authentication at all: admin_propose reaches BlockApi::create_block with no credential extractor, so the property 'only the trusted operator can make the node act'… | unauth-actuation | inside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | `tools/wargame-arms.sh::arm_admin_actuator` — **armed 2026-10-14**, two runs agreeing: an unidentified caller made the node propose — `Success! Block …`, HTTP 200. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N4-02** | Mallory | MORE | One call to the /api/txn actuator exercises more of the node's authority than any propose route: it signs every phase deploy with the validator key AND escrows REV from that key's own vault, up to MAX… | authority-widening | outside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | owed: a test that a caller refused a spend is also refused a propose (one identity for one authority). Today both routes share admin_origin_guard and … | — not armed: the devnet is single-shard, so the gateway route answers 422 before any caller identity is considered — a multi-shard boot is this row's precondition. 
| **W-N4-03** | Mallory | LESS | A node-held key is spent with less protection than its sibling: the faucet signs a REV transfer to a caller-chosen address and is mounted on the PUBLIC listener (permissive CORS, no admin_origin_guard… | unauth-actuation | outside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | owed: Start a node with --dev-mode --deployer-private-key and POST /api/faucet {address} from an arbitrary remote host; the node signs a REV transfer on an … |
| **W-N4-04** | Mallory | AS WELL AS | As well as the operator, a network peer makes the node sign: with attest-on-new-blocks ON BY DEFAULT, a block arriving on the unauthenticated peer transport and passing validation enqueues a propose, … | authority-widening | inside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | `tools/wargame-arms.sh::arm_steady_state` — **armed 2026-10-14**, two runs agreeing: the chain advanced unattended (height 175 → 209): block arrival is itself the proposal trigger. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N4-05** | Mallory | PART OF | The actuator's protection is only partial because it is a bind choice, not a credential: the same mechanism publishes propose+repl+txn+dial on the configured host (0.0.0.0) when api-server.enable-devn… | authority-widening | outside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | owed: Start with api-server.enable-devnet-admin-public=true; a remote host's POST /api/propose is served. Fires when the flag is set; refuted only if the ac… |
| **W-N4-06** | Mallory | OTHER THAN | A party OTHER THAN the operator actuates: admin_origin_guard compares the Origin authority against the request's Host header, which the adversary controls under DNS rebinding, so a rebound page (Origi… | unauth-actuation | inside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | `tools/wargame-arms.sh::arm_admin_actuator` — **armed 2026-10-14**, two runs agreeing: a forged Origin/Host pair passed the guard and the node acted. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N4-07** | Mallory | REVERSE | The actuation direction is reversed: /api/v1/ocapn/dial makes the node itself ORIGINATE an outbound connection and fetch from a peer the caller names (an SSRF actuator) rather than the caller acting o… | unauth-actuation | outside | admin reachable (local / published / DNS rebind) | CONFIRMED | — | owed: With enable-ocapn-dial=true, POST /api/v1/ocapn/dial a sturdyref whose transport/hint is a loopback or internal address; the node dials it. Fires when… |
| **W-N4-08** | Operator | LATE | The node's own safety halt does not stop actuation: after AUTOPROPOSE_MAX_CONSECUTIVE_FAILURES the autopropose timer breaks and sets `autoproposeTimerHalted`, but the block-validated autopropose tap k… | observability-gap | inside | admin reachable (local / published / DNS rebind) | CONFIRMED | R2 | owed: Drive 3 consecutive self-validation failures so `autoproposeTimerHalted` reads true, then deliver one remote block; the tap enqueues a propose and a b… |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Mallory | The admin actuator is not on any network-reachable surface, so Mallory's inject/modify/replay/MITM has no path to the node's own authority. Barriers, each re-derived: (1) the admin HTTP listener that mounts the actuating routes (/api/propose, the /api/txn REV-… |

### W-N5 — validator key lifecycle

**Design intent.** the signing key exists only where the operator intends and can be replaced.
**Census row tested.** key compromise
**Guide words.** row: AFTER, AS WELL AS, BEFORE, LATE, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: EARLY (folded into LATE — the in-memory residen…), LESS (No bounded quantity to decrease: the sec…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N5-01** | Root | NO | The operator's key replacement does not take effect. resolve_validator_private_key returns early whenever casper.validator-private-key (hex) is already Some, so a hex planted in the file layer silentl… | supply-chain-integrity | outside | same-host (key file / argv / env) | CONFIRMED | — | owed: a layered-config test that sets casper.validator-private-key in the file layer AND --validator-private-key-path on the command line and asserts … |
| **W-N5-02** | Root | MORE | The validator private scalar is materialised in one more place than the operator chose: the process argument list. --validator-private-key hex carries the raw key, and hide = true removes it from --he… | disclosure | outside | same-host (key file / argv / env) | CONFIRMED | — | `node/src/configuration/commandline/options.rs::the_validator_key_cannot_be_given_on_the_command_line` — **armed**: the argv route is refused at parse, so no 64-hex scalar can reach `/proc/<pid>/cmdline` (the falsifier's literal read is moot: the route is gone). |
| **W-N5-03** | Root | AS WELL AS | Alongside the keyfile, the secret that unlocks a PEM key is taken from the process environment: get_validator_password reads RNODE_VALIDATOR_PASSWORD before falling back to the console, so an encrypte… | key-at-rest | outside | same-host (key file / argv / env) | CONFIRMED | — | owed: a test/measurement that a PEM keyfile path cannot be unlocked from the environment (e.g. it fails with an empty environment and a non-tty consol… |
| **W-N5-04** | Root | PART OF | Only part of the at-rest protection is achieved. The private key file is narrowed to mode 0600, but that is a DAC bit a host/root intruder bypasses; and the genesis ceremony writes each generated vali… | key-at-rest | outside | same-host (key file / argv / env) | CONFIRMED | — | The existing DAC test the_private_key_file_is_owner_only (crypto/src/util/key_util.rs:262-296) passes while the file remains plaintext-readable by roo… |
| **W-N5-05** | Root | OTHER THAN | A key other than the operator's intended identity signs, and the node never checks. The signing identity is derived from whatever private scalar is in config; the configured casper.validator-public-ke… | authority-widening | outside | same-host (key file / argv / env) | CONFIRMED | — | owed: a startup check (and its test) that the public key derived from the configured private key equals casper.validator-public-key, refusing boot oth… |
| **W-N5-06** | Root | REVERSE | The 'can be replaced' half reverses: because the key is re-resolved from config on every start with no integrity anchor, the replacement is available to the intruder, not only the operator. Root overw… | supply-chain-integrity | outside | same-host (key file / argv / env) | NOT-RE-ESTABLISHED | — | owed: a test that a second boot with a changed key file either refuses or emits a loud identity-change record. None exists; the resolver holds no pers… |
| **W-N5-07** | Root | AFTER | The only binding check runs after the key has already become the signing identity, and only on the propose path. is_active_validator is evaluated inside do_propose (after ValidatorIdentity::from_hex a… | observability-gap | outside | same-host (key file / argv / env) | CONFIRMED | — | `casper/src/blocks/proposer/proposer.rs::a_dag_that_answers_not_bonded_is_refused_with_its_own_sentence` — **armed**: an identity the active set does not list is refused with the *active set* sentence and not the *cannot decide* one — the other arm of the refusal the neighbouring test pins, so a store fault cannot read as a bonding fact. |
| **W-N5-08** | Root | LATE | The secret is destroyed later than intended — in practice never, for the operator-facing copies. PrivateKey zeroizes on drop (the intent exists), but the config field and the argv value are ordinary S… | disclosure | outside | same-host (key file / argv / env) | NOT-RE-ESTABLISHED | — | owed: a test scanning the heap after config build to confirm no 64-char hex scalar remains. The only scrub is PrivateKey::drop (crypto/src/private_key… |
| **W-N5-09** | Root | BEFORE | The key file is adopted before — in fact without — any check on its protection. load_validator_private_key reads any file at the path and returns the scalar unconditionally: no owner check, no mode ch… | observability-gap | outside | same-host (key file / argv / env) | CONFIRMED | — | `node/src/configuration/configuration.rs::validator_private_key_path_is_resolved_into_the_hex_field` — **armed**: a key file readable by group or other is refused when it is loaded. |

### W-N6 — host and container

**Design intent.** a compromise of the node process is contained by the host.
**Census row tested.** — (new surface)
**Guide words.** row: AS WELL AS, EARLY, LESS, MORE, NO, OTHER THAN, PART OF · vacuous: AFTER (no bounded property — a 'after' relation…), BEFORE (no bounded property — a 'before' relatio…), LATE (folded into PART OF — the only ordering …), REVERSE (folded into OTHER THAN — the reverse-dir…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N6-01** | Root | NO | The node performs no containment at all: it runs with the container's default privilege set inside a read-write host mount, and the workspace contains no setuid/setgid/chroot/seccomp/unshare/no_new_pr… | authority-widening | outside | same-host / container | NOT-RE-ESTABLISHED | — | owed: no test asserts a containment property. Falsifier = `docker inspect` on a devnet container showing CapAdd/ReadonlyRootfs/SecurityOpt empty (devi… |
| **W-N6-02** | Root | MORE | MORE actuation surface than the node's own default: the admin HTTP API, which options.rs documents as loopback-only, no-authentication and block-production-triggering ('Off by default ... that API has… | unauth-actuation | outside | same-host / container | CONFIRMED | — | `tools/wargame-arms.sh::arm_admin_actuator` — **armed 2026-10-14**, two runs agreeing: this host reached the admin actuator over its published port and the node acted. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N6-03** | Root | LESS | The data directory is LESS restricted than the key-file discipline the crypto layer applies: it is created with std::fs::create_dir under the process umask and with no explicit mode, so it can land gr… | key-at-rest | outside | same-host / container | CONFIRMED | — | `node/src/runtime/node_environment.rs::the_data_dir_is_created_owner_only` — **armed**: the data directory is created 0700 regardless of the process umask. |
| **W-N6-04** | Root | AS WELL AS | AS WELL AS the argv hex, the keyfile password travels on a second host-readable channel: RNODE_VALIDATOR_PASSWORD in the process environment. /proc/pid/cmdline and /proc/pid/environ are two distinct O… | disclosure | outside | same-host / container | CONFIRMED | — | owed: armed: `tr '\0' '\n' < /proc/rnode-pid/environ / grep RNODE_VALIDATOR_PASSWORD` prints the password for a same-host non-owner uid; there is no arm in … |
| **W-N6-05** | Root | PART OF | Owner-only 0600 at-rest protection covers only PART of the node's key material. The TLS identity key is written 0600, but the consensus validator signing key is never written to a protected file on th… | key-at-rest | outside | same-host / container | CONFIRMED | — | owed: a check that every consensus key the node uses is backed by a 0600 file; the devnet has none — the falsifier is the absence of any `--validator-… |
| **W-N6-06** | Root | OTHER THAN | The validator signing key reaches the node by a channel OTHER THAN the one the source itself recommends. The code documents --validator-private-key-path as the way to 'keep the secret off the command … | disclosure | outside | same-host / container | CONFIRMED | — | `node/src/configuration/commandline/options.rs::the_validator_key_cannot_be_given_on_the_command_line` — **armed**: same pin as W-N5-02: the argv route is gone, and `tools/devnet.sh` seeds the key to a path. |
| **W-N6-07** | Root | EARLY | A supply-chain compromise enters EARLY — at image build, before any runtime containment could act. Both Dockerfile stages are pinned by tag only and not by digest, so a repointed debian:trixie-slim or… | supply-chain-integrity | outside | build-time | CONFIRMED | — | `tools/check-workflow-pins.sh::Dockerfile` — **armed**: the gate opens every `Dockerfile*` and fails a `FROM` ref that is not digest-pinned. |

### W-N7 — build and release provenance

**Design intent.** the artefact an operator runs is what the source builds, verifiably.
**Census row tested.** — (new surface)
**Guide words.** row: AS WELL AS, LESS, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: AFTER (folded into NO. The only 'after' (an ope…), BEFORE (folded into REVERSE. The one genuine ord…), EARLY (No bounded temporal property to deviate …), LATE (No bounded temporal property. The only '…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N7-01** | Root | OTHER THAN | The commit the binary reports is not the commit it was built from: an environment variable read by build.rs takes precedence over `git rev-parse HEAD` and returns early, and the runtime only embeds an… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | `node/build_provenance.rs::an_explicit_commit_that_disagrees_loses_to_git_and_is_flagged` — **armed**: a `GIT_HEAD_COMMIT` that disagrees with git HEAD loses to git and the disagreement is flagged. |
| **W-N7-02** | Root | NO | Nothing verifies the running artefact against anything. The node's only `verify-*` subcommands act on chain data (a block JSON, an attestation) via this node's own hashers — never on the binary — and … | observability-gap | outside | build / release channel | CONFIRMED | — | owed: replace one byte in the shipped `rnode` (or bind-mount a swapped binary) and start it — it boots and answers the same /version; no control refus… |
| **W-N7-03** | Root | MORE | The build consumes more than the source names: the Dockerfile exposes build args (RUSTFLAGS, CARGO_FEATURES) that the builder can set to inject arbitrary rustc flags or crate features into the compile… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | owed: Build the image twice — default vs `--build-arg RUSTFLAGS='-C metadata=evil'` (or `--cfg flag`) — and compare `sha256sum` of the produced `rnode`; the… |
| **W-N7-04** | Root | LESS | The image build applies less determinism than the property needs: the Dockerfile invokes `cargo build` without `--locked`, whereas the CI release build uses `--locked`. The artefact the operator actua… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | owed: a gate asserting every build path passes `--locked`. Positive instrument: desync the lock (`cargo update -p dep --precise other`) and run the Do… |
| **W-N7-05** | Root | AS WELL AS | As well as the pinned workflow actions, the build carries an unpinned supply-chain input the gate never reads: both container bases are referenced by mutable tags, not content digests, and the pin che… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | Run `tools/check-workflow-pins.sh` — it reports clean and never opens docker/rnode/Dockerfile; a mutable `FROM debian:trixie-slim` passes unremarked. … |
| **W-N7-06** | Root | PART OF | Only part of an integrity chain exists: the release artefact's sole token is a sha256 computed and uploaded from the same job and same channel as the binary. Root, able to replace the release artefact… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | owed: Obtain the artefact and its digest through the one channel (`gh run download`); substitute both binary and rnode.sha256 in that store — a consumer rea… |
| **W-N7-07** | Root | REVERSE | The reference is inside the thing referenced: the gate that is meant to hold the node's vendored genesis contracts to the upstream oracle compares each embedded `.rho` to a copy that lives in this rep… | supply-chain-integrity | outside | build / release channel | CONFIRMED | — | owed: Edit casper/src/genesis/resources/Registry.rho and casper/src/genesis/vendored-originals/Registry.rho with the same change; `tools/audit-vendored-sour… |

### W-N8 — OCapN transports

**Design intent.** the capability wire lets a stranger do only what a stranger is meant to do.
**Census row tested.** — (new surface)
**Guide words.** row: AS WELL AS, EARLY, LESS, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: AFTER (No bounded post-terminal property deviat…), BEFORE (Folded into NO — the absent precondition…), LATE (Folded into OTHER THAN — the effect of t…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N8-01** | Mallory | NO | The node actuates its own authority for a caller it cannot name: a CapTP delivery is turned into a deploy signed by the node's own deployer key — signed_invoke(&term, &self.key, ...) at node/src/api/o… | unauth-actuation | inside | OCapN transport enabled | CONFIRMED | — | owed: Owed: fetch `rho:rchain:revVault/getBalance` over tcp-testing-only and deliver an argument; assert the resulting deploy is NOT signed by the node's de… |
| **W-N8-02** | Mallory | MORE | The spend bound is node-wide, not per-peer, so one stranger drives MORE of the node's REV than a single peer is meant to. BRIDGED_DEPLOYMENTS_PER_SEC = 4 (node/src/api/ocapn.rs:126) is a single RateLi… | resource-amplification | inside | OCapN transport enabled | CONFIRMED | — | owed: Owed: a two-session starvation test — one session loops deliveries to a published capability while a second sends one; assert the honest session still… |
| **W-N8-03** | Mallory | LESS | The node's advertised peer surface provides LESS than the capability model assumes: `websocket` carries CapTP in cleartext with no integrity. wss:// is 'implemented but not reachable from the node's c… | plaintext-transit | inside | OCapN transport enabled | CONFIRMED | — | owed: Owed: a MITM probe — interpose a TCP relay between the node's websocket listener and a peer and assert the CapTP records (including a chain-capability… |
| **W-N8-04** | Mallory | AS WELL AS | A delivery to the ERTP capability (ERTP_SWISS = b"rho:rchain:ertp", node/src/api/ocapn.rs:85) does more than answer. Because its method rides the message, the node runs invoke_member_term (node/src/ap… | authority-widening | inside | OCapN transport enabled | CONFIRMED | — | owed: Owed: deliver `[makeIssuerKit "X"]` to rho:rchain:ertp over tcp-testing-only and assert no new registry URI is minted under the node's key and no capa… |
| **W-N8-05** | Mallory | PART OF | A part is treated as the whole: the ONLY admission check proves possession of the peer's session key — peer.location_signature_is_valid() at ocapn/src/conn.rs:414, which covers nothing but the peer's … | authority-widening | inside | OCapN transport enabled | REFUTED | — | owed: Owed: a continuity test — reconnect as one node identity across two sessions and assert the wire sees one peer. Today each ephemeral session key is a … |
| **W-N8-06** | Mallory | OTHER THAN | A session is named by a field other than the key its transport proved. accept_and_book computes book_key from session.verified_peer_key() (ocapn/src/owner.rs:284) and, when that is None — which it is … | authority-widening | inside | OCapN transport enabled | CONFIRMED | — | `ocapn/tests/session_owner.rs::a_session_is_booked_under_the_name_its_transport_proved` — **armed**: a session is held under the key its transport proved, not under the designator the peer asserts. |
| **W-N8-07** | Mallory | REVERSE | The roles invert: a stranger makes the node dial outward as a client. Fetching the sturdyref enlivener (fixtures::STURDYREF_ENLIVENER, ocapn/src/fixtures.rs:29) and delivering a sturdyref makes the no… | authority-widening | inside | OCapN transport enabled | REFUTED | — | `ocapn/src/dial_policy.rs::link_local_and_the_metadata_endpoint_are_refused` — **armed**: the dial policy refuses link-local and metadata-endpoint targets (the row is REFUTED: the property holds). |
| **W-N8-08** | Mallory | EARLY | The actuation precedes the check that guards it. handle_deliver resolves the target and runs target.deliver_in(...) at ocapn/src/conn.rs:761 — for a chain capability that executes check_deploy_budget … | unauth-actuation | inside | OCapN transport enabled | CONFIRMED | — | owed: Owed: send a delivery to a chain capability at a reused `answer_pos` and assert the node's submitted-deploy count is unchanged. Today the deploy is su… |

### W-N9 — validator powers

**Design intent.** a bounded validator can neither split safety nor stall liveness.
**Census row tested.** all census rows
**Guide words.** row: AS WELL AS, BEFORE, LATE, LESS, MORE, NO, OTHER THAN, REVERSE · vacuous: AFTER (The only after-the-fact mechanism is the…), EARLY (folded into AS WELL AS — 'produced befor…), PART OF (folded into LESS — the round gate's one-…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N9-01** | Alice | NO | The honest guard withholds its attestation outright once the unfinalised deploy's age passes ATTESTATION_HORIZON: attestation_inputs returns new_state_transition=false, and with quorum_reachable false… | recovery-unbuilt | inside | in-model (the bounded validator) | REFUTED | — | owed: a devnet (or fixture) that holds one deploy-bearing fringe unmoved for >15 heights and asserts the honest validator re-attests. `but_a_node_quie… |
| **W-N9-02** | Alice | MORE | An honest node emits more attestations than the pace bound the guard exists to impose. When quorum_reachable, the `if !paced { return false; }` arm licenses an attestation on every explicit propose re… | resource-amplification | inside | in-model (the bounded validator) | CONFIRMED | — | `tools/wargame-arms.sh::arm_admin_actuator` — **armed 2026-10-14**, two runs agreeing: 5 of 5 explicit proposes produced a block (height 3 → 8): a caller-driven propose is not paced, which is deliberate (C196). The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N9-03** | Alice | LESS | The guard and the finalizer measure the quorum against potentially different bonded maps, so an honest node can under-count the reachable stake and withhold an attestation the finality rule actually n… | recovery-unbuilt | inside | in-model (the bounded validator) | CONFIRMED | — | owed: assert runtime.compute_bonds(pre_state_hash) equals the bonds_map the finalizer is handed (multi_parent_casper.rs:377-412) at every height of an… |
| **W-N9-04** | Alice | REVERSE | The node performs the precise opposite of the one-block-per-round rule the H-1 gate enforces: with `--equivocation-injection` armed it signs a second, content-distinct block at the same (sender, seq_n… | unauth-actuation | outside | dev flag (--equivocation-injection) | CONFIRMED | — | owed: measurement: start a node with the flag and assert every peer records/slashes the twin — the twin's body is stored by put_block at proposer.rs:284 pre… |
| **W-N9-05** | Alice | LATE | The node proposes LATER than the round permits, on a trigger that is the host wall clock rather than chain state. After ROUND_STALL_ESCAPE (15 s) blocked — or LIVENESS_WINDOW attempts — round_escape_o… | clock-entropy-substrate | inside | in-model (the bounded validator) | CONFIRMED | — | owed: step the host clock forward/backward by >=15 s while the round gate is held and observe whether the escape line (:822-828) fires and whether the… |
| **W-N9-06** | Alice | OTHER THAN | A slash is justified by a route OTHER THAN a slashable justification — equivocation evidence — and that route is adjudicated against the receiving node's own DAG completeness, so the same block is val… | observability-gap | inside | in-model (the bounded validator) | CONFIRMED | — | owed: two nodes at different DAG depths validate the same slashing block and must reach one verdict; `equivocation_evidence_is_proved_only_by_a_signed… |
| **W-N9-07** | Alice | BEFORE | The attestation licence is granted on an input that exists BEFORE the work it licenses: work_age_floor counts the unfinalised work's age from the parents' fringe top (fringe_top, from pre_state.prev_f… | resource-amplification | inside | in-model (the bounded validator) | CONFIRMED | — | `casper/src/blocks/proposer/proposer.rs::late_work_below_the_fringe_is_counted_from_the_fringe` — **armed**: the licence is granted from the fringe: `work_age_floor` and `attestation_inputs` are computed from the fringe, not the tip. |
| **W-N9-08** | Oscar | AS WELL AS | Besides its honest blocks, a validator emits a signed claim of finality it is not entitled to make. The finality-attestation endpoint signs `blake2b256(tag//nonce//block_hash)` over its own `last_fina… | authority-widening | inside | in-model (the bounded validator) | NOT-RE-ESTABLISHED | — | owed: a recovery test that one lying attester cannot pin an anchor the network never finalised; /home/patrick/RNodeRust/node/src/runtime/recovery_veri… |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Oscar | Oscar reaches the gates but cannot EFFECT the statement's safety failure (split safety). Barrier 1: the H-1 gate at insert refuses a second block by the same sender at the same seq_num, recording the header as slash evidence and returning BEFORE any partial wr… |
| Oscar | Oscar reaches the gates but cannot EFFECT the statement's liveness failure (stall liveness). Barrier 1: the round boundary the proposer builds on retires a silent bonded sender once LIVENESS_WINDOW heights pass, on the same clock the window runs, so a withheld… |

### W-N10 — observability and alerting

**Design intent.** an operator can see production, finality and halt state.
**Census row tested.** — (new surface)
**Guide words.** row: LESS, MORE, PART OF, REVERSE · vacuous: AFTER (folded into REVERSE — the surface reflec…), AS WELL AS (folded into MORE — the extra component r…), BEFORE (The only pre-event-reading candidate, CU…), EARLY (No writer pre-writes its condition: note…), LATE (The sustained-stall flag is deliberately…), NO (Every intended observation (production, …), OTHER THAN (folded into REVERSE — the halt field's v…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N10-01** | Operator | REVERSE | The halt observation reverses to a false all-clear on the operator's prescribed recovery. `autopropose_timer_halted` is an `ArcAtomicBool` whose only writer in the whole workspace is store(true) (note… | observability-gap | outside | no adversary (failure mode) | CONFIRMED | R2 | `casper/tests/reset.rs::a_halt_written_before_the_restart_is_read_at_the_next_boot` — **armed**: a halt written before a restart is read back and seeded at the next boot, so the all-clear is the durable record's and not the process's. |
| **W-N10-02** | Operator | LESS | Finality (and sync) counters understate across the operator's restart. STALL_EPISODES, NON_QUIET_MERGE_REPORTS and UNRESTORABLE_RECORDS — and the stall reason/sustained cells — are process-global stat… | observability-gap | outside | no adversary (failure mode) | CONFIRMED | R2 | `casper/tests/reset.rs::the_shard_health_counters_survive_a_restart` — **armed**: the stall-episode, non-quiet-merge and unrestorable counters are persisted on the transition and re-seeded after a restart. |
| **W-N10-03** | Operator | MORE | The finality surface attributes one shard's events to the node. STALL_EPISODES / NON_QUIET_MERGE_REPORTS / UNRESTORABLE_RECORDS and CURRENT_STALL_REASON are process-wide and NOT keyed by shard, unlike… | observability-gap | inside | no adversary (failure mode) | CONFIRMED | R2 | `node/src/api/shard_routing.rs::health_by_shard_reports_every_member_and_the_non_primarys_halt` — **armed**: each member's finality counters are its own — the map carries 3 and 9 for two shards, not one process-wide number. |
| **W-N10-04** | Operator | PART OF | Only the primary shard's halt state reaches the surface. ShardRoutingBlockApi::proposer_health delegates to primary_api() alone, so a non-primary shard whose autopropose timer has halted is invisible … | observability-gap | inside | no adversary (failure mode) | CONFIRMED | R2 | `node/src/api/shard_routing.rs::health_by_shard_reports_every_member_and_the_non_primarys_halt` — **armed**: a non-primary shard's halt reaches the surface through the per-shard map, and `node/src/api/conversion.rs::the_per_shard_map_carries_each_members_halt` pins the wire shape. |

### W-N11 — upgrade, migration and DR

**Design intent.** the operator can advance and restore the node without a wipe.
**Census row tested.** silence of a quorum; operator error
**Guide words.** row: AS WELL AS, BEFORE, EARLY, LATE, LESS, MORE, NO, OTHER THAN, PART OF, REVERSE · vacuous: AFTER (no bounded property to deviate from: the…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N11-01** | Root | BEFORE | At boot the node reads a durable reset record out of its own store, and that on-disk record — not the operator's `--sync-anchor` — chooses where the node rewinds and re-roots its runtime. When both ar… | unauth-actuation | outside | same-host / operator input | CONFIRMED | — | `casper/tests/reset.rs::a_reset_record_that_contradicts_the_named_anchor_refuses_boot` — **armed**: a store reset record that contradicts `--sync-anchor` refuses the boot rather than silently overriding the operator. |
| **W-N11-02** | Root | PART OF | The advance's only data migration — the deployer-index backfill — is armed or suppressed by an unauthenticated on-disk cursor. A host that sets `DEPLOYER_INDEXED_FROM_KEY` to 0 (the 'index complete' v… | observability-gap | outside | same-host / operator input | NOT-RE-ESTABLISHED | — | `casper/src/dag.rs::a_completion_record_that_does_not_describe_the_index_is_not_believed` — **armed**: a `DEPLOYER_INDEXED_FROM_KEY=0` completion with no matching record count is walked, not believed. |
| **W-N11-03** | Root | REVERSE | The advance has no reverse guard. The store carries no format version, so an older binary — or an attacker-supplied image at a base pinned only by tag — runs over a newer store and is accepted: the on… | recovery-unbuilt | outside | same-host / operator input | NOT-RE-ESTABLISHED | — | `node/src/runtime/node_runtime.rs::a_store_from_a_newer_build_is_refused_and_this_build_stamps_what_it_opens` — **armed**: a store-format marker higher than this build refuses boot; an absent or lower one is stamped and read. |
| **W-N11-04** | Root | OTHER THAN | The shipped DR procedure does not restore the operator's own node from a vault; `--restore-from-master` copies another node's raw LMDB environments onto each joiner byte-for-byte, and the joiner then … | supply-chain-integrity | outside | same-host / operator input | CONFIRMED | — | `spec/audit/passes.md::C259` — **armed**: closed as a recorded residual rather than armed: the copy now carries the anchor bound and a manifest of what it adopted (C259, §117), and the signed-genesis walk is what remains owed — the row's own falsifier says no test can assert it. |
| **W-N11-05** | Root | AS WELL AS | As well as advancing the node, every restart re-supplies its signing identity from a plaintext location the host reads: the validator key may be passed as `--validator-private-key hex` on the command … | key-at-rest | outside | same-host / operator input | CONFIRMED | — | `node/src/configuration/commandline/options.rs::the_validator_key_cannot_be_given_on_the_command_line` — **armed**: the flag that put the scalar in `/proc/<pid>/cmdline` is gone: it is refused at parse, which is the stronger property (the route no longer exists). |
| **W-N11-06** | Operator | NO | A node whose last-finalised block is absent from its own height map (the map omits validation-failed blocks) is a state in which NO reset is requested: the decision returns RefuseNoHeight, the detecto… | recovery-unbuilt | inside | operator tooling (failure mode) | CONFIRMED | R3 | `casper/src/multi_parent_casper.rs::an_anchor_the_view_cannot_place_is_placed_from_the_store` — **armed**: an anchor the view cannot place is placed from the store, and the plan then takes the arm that acts. |
| **W-N11-07** | Operator | MORE | --restore-from-master copies the survivor's whole chain state onto every joiner with no re-verification, so the joiners adopt the survivor's view INCLUDING blocks above the agreed anchor that only it … | authority-widening | outside | operator tooling (failure mode) | CONFIRMED | R2 | `tests/reconcile-regression.py::run_restore` — **armed**: `--restore-from-master` refuses to copy blocks above the anchor unless the operator sets `RECONCILE_ADOPT_ABOVE=1`, and names the heights. |
| **W-N11-08** | Operator | LESS | restore_chain_state_from_master leaves every environment the survivor does not hold in place on the joiner: the per-env guard skips the swap when the source or staging lacks the dir, so a joiner-only … | recovery-unbuilt | inside | operator tooling (failure mode) | CONFIRMED | R3 | `tests/reconcile-regression.py::run_restore` — **armed**: a joiner-only environment the survivor lacks is moved aside (`<env>.reconcile-orphan`) rather than left standing. |
| **W-N11-09** | Operator | EARLY | When RECONCILE_ANCHOR is set the tool acts on the operator's named root BEFORE (and instead of) any quorum is established: the `if [ -n "$ANCHOR" ]` branch bypasses the attestation/verification/meet c… | authority-widening | outside | operator tooling (failure mode) | CONFIRMED | R2 | `tests/reconcile-regression.py::run_meet` — **armed**: an operator-named `RECONCILE_ANCHOR` is labelled as the operator's and an anchor nobody holds is refused. |
| **W-N11-10** | Operator | LATE | The reset's two operator-facing sentences disagree on the anchor's height for the same block: the acting arm logs the last-finalised block at height=N (anchor_height), while the dry-run sentence — the… | observability-gap | inside | operator tooling (failure mode) | CONFIRMED | R2 | `casper/src/multi_parent_casper.rs::the_two_rewind_sentences_name_one_height` — **armed**: the acting line and the dry run take their height from one function, and the phrase occurs once in the source. |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Root | The advance/restore decision consumes only two things, neither of which this persona can write. (1) The operator's own argv/env knobs — RECONCILE_ANCHOR, --master, --self-reset, --sync-anchor (tools/reconcile-network.sh:615; node/src/configuration/commandline/… |

### W-N12 — DoS as a dimension

**Design intent.** no reachable action costs the node superlinearly in attacker input.
**Census row tested.** deploy flooding
**Guide words.** row: AS WELL AS, EARLY, LATE, LESS, MORE, NO, PART OF · vacuous: AFTER (Folded into LATE — the same mechanism (a…), BEFORE (Folded into EARLY — the same mechanism (…), OTHER THAN (No mechanism in the cost dimension where…), REVERSE (No bound whose application INCREASES cos…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N12-01** | Mallory | MORE | Term normalization costs O(k^2) in the number of list elements k, not O(k): normalize_send/normalize_contr/normalize_method iterate the element list and insert each normalized element at index 0 of th… | resource-amplification | inside | remote · default config | CONFIRMED | — | `rholang/src/normalizer.rs::the_accumulators_push_and_reverse_rather_than_prepend` — **armed**: the k-vs-2k *ratio* is deliberately not committed: it was written twice, and both versions measured the machine rather than the code — the first read 8.1 here and 21.6 on the runner, the second read the reference at 16.5x here and 2.3x there — so the committed assertion is the *form* the cost follows from (push and reverse once, never prepend), which is decidable exactly and cannot flake. |
| **W-N12-02** | Mallory | NO | The design intent — a bound on what a reachable deploy may cost — has no breadth term at all. `parse` refuses a term by AST depth (MAX_AST_DEPTH) and by flat operator-chain length (MAX_CHAIN_LENGTH), … | resource-amplification | inside | remote · default config | CONFIRMED | — | `spec/audit/passes.md::C289` — **armed**: decided, not armed: a breadth rule in the parser would be **fork-visible** — the oracle accepts a wide term, so refusing one here would reject a block the oracle accepts. The bound that exists is on the wire form of a deploy at this node's ingress (64 KiB, refused before the parse), and the residual is registered (C289, §6) rather than papered over with an element cap. |
| **W-N12-03** | Mallory | EARLY | The deploy is parsed AND fully normalized before the admission decision that would refuse it. `deploy` calls `source_to_adt_with_env` first, then `add_deploy`, which is the step that checks the pool a… | resource-amplification | inside | remote · default config | CONFIRMED | — | `casper/src/dag.rs::a_deploy_the_admission_refuses_is_never_parsed` — **armed**: the ordering is the property: a deploy the admission refuses answers "rate limit" and never "parsing term", so the parse cannot be bought with work the node will discard. The timing half of the falsifier measures the same ordering, which the refusal *message* already decides. |
| **W-N12-04** | Mallory | PART OF | The transport `stream` aggregate byte budget (blobs x max_stream_message_size, the C113 fix) is charged only for `Data` chunks; the charge sits inside the `if let Some(chunk::Content::Data(d))` arm. E… | resource-amplification | inside | remote · default config | CONFIRMED | — | `comm/src/transport/grpc_transport_receiver.rs::a_header_chunk_is_charged_for_the_bytes_a_peer_chooses` — **armed 2026-10-14**: a header chunk is charged for the bytes the peer chooses, so a stream of maximal headers cannot hold budget it did not spend. |
| **W-N12-05** | Mallory | AS WELL AS | The deploy ingress does the intended admission AND retains the whole deploy, including its up-to-16 MiB term, for the pool's lifetime — while the pool's bound is a COUNT (10,000), not a byte budget. T… | resource-amplification | inside | remote · default config | REFUTED | — | owed: Fill the pool with 10,000 max-size deploys and measure RSS against the operator's expectation; a count bound cannot stop ~160 GiB. owed: the measureme… |
| **W-N12-06** | Mallory | LESS | The deploy admission bound is a SINGLE GLOBAL token bucket shared by every client, not per reachable identity. The census row this node is sited on says admitting is bounded per unit time PER reachabl… | resource-amplification | inside | remote · default config | CONFIRMED | — | `tools/wargame-arms.sh::arm_deploy_admission` — **armed 2026-10-14**, two runs agreeing: the second client is admitted while the first floods. The run's numbers and its per-row table are in `spec/audit/evidence/wargame/arms-f9521edd7/results.md`. |
| **W-N12-07** | Mallory | LATE | On the streamed path the wrong-network-id check and the certificate-bound sender check run LATE — inside the reassembly breaker, which executes in `stream_handler::collect` only AFTER the whole chunk … | resource-amplification | inside | remote · default config | CONFIRMED | — | `comm/src/transport/grpc_transport_receiver.rs::a_sender_from_another_network_is_refused_and_not_dispatched` — **armed 2026-10-14**: a sender from another network is refused and never dispatched. |

**Table B — barriers that held.**

| Persona | Barrier |
|---|---|
| Mallory | Reachable, but the property holds: every unauthenticated surface Mallory can inject on bounds its input, and every inherently superlinear algorithm in the node is capped, budgeted, or fixed, so no reachable action's cost grows faster than linearly in Mallory's… |

### W-N13 — entropy and randomness sourcing

**Design intent.** the names a deploy can forge are unpredictable.
**Census row tested.** adjacent to key compromise
**Guide words.** owed: NO, MORE, LESS, AS WELL AS, PART OF, OTHER THAN, REVERSE, EARLY, LATE, BEFORE, AFTER

**No rows.** No persona reaches this function (Alice, Bob, Eve, Mallory, Operator, Oscar, Root). See the method note.

### W-N14 — clock and time

**Design intent.** the node's notion of time is bounded and observable.
**Census row tested.** a validator rejecting its own block
**Guide words.** row: AS WELL AS, NO, PART OF, REVERSE · vacuous: AFTER (folded into REVERSE — the age comparison…), BEFORE (No bounded property is deviated: the sta…), EARLY (folded into REVERSE — an early (future-d…), LATE (folded into REVERSE — a late (past-dated…), LESS (The inner/past side of the one-sided pre…), MORE (folded into NO — a value larger than the…), OTHER THAN (folded into NO — a value other than a co…)

| ID | Persona | Word | Deviation | Class | In-model | Precondition | Verdict | Disposition | Falsifier |
|---|---|---|---|---|---|---|---|---|---|
| **W-N14-01** | Oscar | NO | No validation and no bound exists for the block header's `timestamp`: `block_summary` runs justification-regression, sequence-number, block-number and a pure-check array (shard id, block-number-based … | clock-entropy-substrate | inside | in-model (the bounded validator) | CONFIRMED | — | `casper/src/validate.rs::a_block_stamped_before_its_parents_is_refused` — **armed**: the bound is one-sided and the test says so. A block stamped a day *after* its parents is accepted (the backlog case — C289's deliberately registered deviation, §119) and one stamped past the drift *behind* them is `InvalidBlockTimestamp` on the acceptance path. The row's `i64::MAX` stamp is the accepted direction, asserted rather than described. |
| **W-N14-02** | Oscar | REVERSE | The one age bound that does exist points only one way. `attest_is_live` is `now_ms.saturating_sub(block_ms) <= ATTEST_MAX_BLOCK_AGE_MS`, so a future stamp makes the subtraction negative and every futu… | resource-amplification | inside | in-model (the bounded validator) | CONFIRMED | — | `node/src/runtime/node_runtime.rs::a_block_stamped_far_in_the_future_is_not_answered` — **armed**: the forward half of the window: a block stamped beyond the skew, `i64::MAX` or `i64::MIN`, is not a live round. |
| **W-N14-03** | Oscar | AS WELL AS | The same unvalidated proposer value is republished verbatim, as the block's canonical time, to every Rholang contract through `rho:block:data` and to every client through the block API. The contract c… | observability-gap | inside | in-model (the bounded validator) | CONFIRMED | — | owed: Two blocks identical except `timestamp`, evaluated by the same contract reading rho:block:data: the contract's observed time tracks the proposer's fie… |
| **W-N14-04** | Oscar | PART OF | Only part of the node's time surface is bounded. The age bound gates the attestation tap alone — the code's own note says a deploy and the not-due retry still propose — while the contract-read path (r… | observability-gap | inside | in-model (the bounded validator) | CONFIRMED | — | owed: Live: hand a catching-up node a deploy-bearing block older than ATTEST_MAX_BLOCK_AGE_MS and observe a proposal is still produced from it (the comment … |

### Referee calibration

| Control | Node | Expected | Verdict |
|---|---|---|---|
| known-true | N4 | CONFIRMED | CONFIRMED |
| false | N4 | REFUTED | REFUTED |
| out-of-model | N4 | NOT-RE-ESTABLISHED | NOT-RE-ESTABLISHED |
| known-true | N2 | CONFIRMED | CONFIRMED |
| false | N3 | REFUTED | REFUTED |
| out-of-model | N9 | NOT-RE-ESTABLISHED | NOT-RE-ESTABLISHED |

## Table E — the statement test

The census above is not left standing: the wargame adjudicates it. A row the wargame contradicts is
**edited above**, not merely noted below — this is the table that makes the section a test.

| Census row | Wargame verdict | Basis |
|---|---|---|
| **Equivocation** — detectable, no `slash` | **holds** | W-N9-04: `--equivocation-injection` signs a second block at one `(sender, seq_num)` and every peer records it as slash evidence — detectable, as the census says. The census's *cost* cell ("there is no `slash`") is confirmed: detection carries no penalty. |
| **Withholding and selective delay** | **holds** | No new divergence was found: W-N1's merge is on the chain, and the barriers at N1 and N9 held (Table B). The census's "liveness under partition, not measured here" cell is untouched by a `read`-class exercise. |
| **Silence of a quorum** | **untested** | The census's own falsifier is *"stop two of four, restore both"* — a live run. A wargame over the code cannot supply it; the row stays a measurement owed to a committed run, not to this section. |
| **Deploy flooding** | **holds, and the census is honest** | The census already names the gap in its *"what does not bound it"* cell — *"the rate from one identity"*. The wargame confirms the mechanism and widens it: the bound is **one process-global token bucket**, not per identity, on both surfaces (W-N2-01/08, W-N12-06), and the pool cap is a count, not a byte budget (W-N2-03). |
| **State divergence at a fringe key** | **holds** | No row re-derives a producer; the store is gone. |
| **A validator rejecting its own block** | **holds** | No wargame row re-derives the self-reject; the nearest, W-N9-01, is `REFUTED`. |
| **Operator error during recovery** | **underclaims** | W-N11-01: at boot the node reads a **durable on-disk reset record** that overrides the operator's `--sync-anchor` — a recovery path the census's "operator error" row, which names only the operator's *choice* of anchor, does not describe. |
| **Key compromise** | **underclaims** | The census bounds it by the stake assumption. The wargame finds the *host* path to the same key is outside the statement entirely and unguarded: an unencrypted PEM is accepted at the path (W-N5-01/02), the hex rides `argv`, the password rides the environment (W-N6-04), and the key file's mode is never checked on read (W-N5-09). |
| **Post-quantum** | **out of scope (correct)** | W-N9's out-of-model control returned `NOT-RE-ESTABLISHED` as it should. |

**What this says.** Two census rows are **too narrow** for the surface the node actually runs on:
*operator error during recovery* omits the durable-record path, and *key compromise* omits the host. Both
are rows where the *statement* should be widened, not rows where the node is broken — which is the honest
form of the finding, and the reason the bound is stated rather than assumed.

## What the preconditions add up to

**The `remote peer · any valid cert` row was adjudicated on 2026-10-13 and the seam is deliberately not
built** (register C290, `spec/audit/passes.md` §116). The remedy the rows imply — a bonded-identity
allow-list at the packet dispatch — cannot be built: the subject there is the node address a
certificate proves, a bond is keyed by a 65-byte secp256k1 `Validator`, and **nothing in the tree maps
one to the other**. The sound route (certify the validator key in the node certificate) is a
certificate-format change *and* still needs the bonded set published into `comm`; the cheap route (an
operator-maintained address list) goes stale as the validator set moves; and a peer announcing its own
validator key is unsound, because that announcement is the thing an attacker forges. **What the ten rows
retain is resource cost, not authority:** a stranger may spend the node's bandwidth and CPU up to the
dispatch and stream bounds, and may not have a block accepted — `deploy_signatures`, the bonds cache and
the merge's rejections are the authorization boundary, and none of them reads the transport's identity.
Read §116 before re-proposing it; that section also names what would change the decision.

| precondition | rows | what it means |
|---|---|---|
| `remote · default config` | 17 | an exploit on an unconfigured node |
| `remote peer · any valid cert` | 10 | remote — the transport admits any keypair, no bonded-peer allow-list |
| `remote · passive read` | 5 | disclosure to an on-path reader |
| **subtotal — no precondition** | **32** | **exploits on a default node** |
| `in-model (the bounded validator)` | 11 | the statement's ≤1 Byzantine validator does it |
| `admin reachable (local / published / rebind)` | 8 | needs a misconfig, or a same-host/browser actor |
| `OCapN transport enabled` | 8 | needs an opt-in |
| `same-host (key file / argv / env)` + `/ container` + `/ operator input` | 20 | needs host access — **outside the statement** |
| `build / release channel` + `build-time` | 8 | needs a build-time adversary |
| `operator tooling` + `no adversary` | 9 | **failure modes, not exploits** |
| `dev flag (--equivocation-injection)` | 1 | a test flag |

**The number that matters: 32 of 97 rows are exploits on a default node.** The rest are conditional, and
a row with no precondition is not in the table at all — which is the discipline `check-wargame.sh`
enforces.

## Table F — the join

The referee is blind to the register; the join happens here. Each `CONFIRMED` finding is labelled `new`
(no register row covers it) or `known (C<n>)`.

| | Count |
|---|---|
| `CONFIRMED` findings | 85 |
| **new** (no register row) | **≈36** |
| **known** (a register row covers it) | **≈49** |

**The hit rate is the number criterion 3 wants**, and it is the exercise's most informative single output:
a `known` finding is evidence the register's coverage works, a `new` one evidence of a gap. *This join is
a first pass* — it matches each finding's cited source path against the register's evidence cells and does
not adjudicate *which* row; a later pass should read each match. The near-half that are `known` is the
expected shape for a surface the September audit and the OCapN HAZOP already crossed.

## Table G — provenance

| | |
|---|---|
| Reviewed revision | `dev` at `1419b127d` |
| Method | `read` — no live multi-node attack was run; every falsifier is `owed` unless a committed test arms it |
| Run | `spec/audit/evidence/wargame/run-1419b127d/` — script, journal, ledger |
| Pre-registration | `spec/audit/evidence/wargame/preregistration.md` |

**Calibration.** controls caught: 6 of 6 · register-leaks: 0 · vacuity: 47%

**The row citations were re-derived on 2026-10-13**, after the remediation PRs #350–#352 moved code
this table points into. The *verdicts* are as of `1419b127d` and are not restated — **and neither is the
row text**: a row says what the node did at that revision, which is the finding, so a row that reads
"the bound is not per identity" is a true statement about the tree the wargame read and not a claim
about today's. The `file:line` references are as of the current tree, so a reader can still open the
site a row is about. Where a fix made a row's `owed:` falsifier the wrong question — the per-deployer
pool share (W-N2-04) means one identity can no longer fill the pool the row asked to fill — the register
carries the closure, not this table. Two adjudications worth reading before re-proposing anything here:
the published-status split (W-N3-01) and the peer-admission decision (§116 of `spec/audit/passes.md`,
register C290), the second of which explains why the ten `remote peer · any valid cert` rows are
deliberately not closed.

**What this section can and cannot claim.** It supplies a **class vocabulary** and a **statement
verdict**. It **cannot supply a rate** — that is the register's (rows opened and closed per window), and a
wargame over the code is a `read`-class instrument. And the referee's independence is a prompt-level
claim the seeded controls *measure* rather than prove: they caught 6 of 6 here, which is why the verdict
column is reported at all — but the same exercise's first two pilots each over-confirmed a control, so
the calibration is the load-bearing number and is quoted every run.
