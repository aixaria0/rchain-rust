# N=8 broadcast-handoff hypothesis — bounded transport follow-up

Status: **experimental; no release or merge acceptance**.

## Pinned evidence and negative controls

- Standalone quorum-clock PR #407 source `940165cd062391e27f6a016ee4af3e0b82996902`: [Rust run 39640484254](https://github.com/aixaria0/rchain-rust/actions/runs/39640484254) succeeded; [canonical N=8 39640491759](https://github.com/aixaria0/rchain-rust/actions/runs/39640491759) failed first partition cycle at finalized height 57 -> 59 (required >=60 in 300 seconds).
- Combined quorum-clock + recent-user-work experiment `63ff68dc78659c8b00f038625485a5195f86a28a`: [Rust 39642254299](https://github.com/aixaria0/rchain-rust/actions/runs/39642254299) succeeded; [canonical N=8 39642261751](https://github.com/aixaria0/rchain-rust/actions/runs/39642261751) failed at the same 57 -> 59 threshold.
- Previous standalone `b9324947` [39639570219](https://github.com/aixaria0/rchain-rust/actions/runs/39639570219) is *historical*, not a passing final build.
- Both canonical runs preserve the marker/original volumes but do not complete two cycles or show unconditional eight-node rejoin-finality/hash equivalence. A rotating diagnostic PASS cannot replace these criteria.

## Accepted DAG and transport discriminator

In `final-gap-real-nodes-39640491759/final-gap/n8-partition/partition-1-before-healing/causal/node-*.json` and the corresponding `39642261751` captures, the seven online nodes have divergent per-sender latest views. For example, node 0 observes six live senders near 69 while one sender reaches 97; nodes 1–6 separately observe their own 94–97-height contributions even when another node's view of those same senders remains near 69. These are **accepted DAG snapshots**, not proof that those packets were never sent. All capture samples are source/run-pinned and capture-time-sensitive.

The observed connection topology is hub/spoke (seven peers at the hub, one at each spoke). `undeliveredSends` is **not a network packet drop counter**: `shared/src/chan.rs` counts errors at internal channels and explicit best-effort sinks since process start. A high value by itself does not diagnose peer transport failure.

A separate concrete source defect exists: `CommUtil::send_block_hash` called `send_to_peers` and always logged "Sent hash ...", while `send_to_peers` discards the `Vec<CommErr<()>>` returned by `TransportLayer::broadcast`. The original code cannot distinguish full, partial, or failed transport handoff. The experimental correction reads per-peer results, retries failed/unreported targets once under a two-second cap, and records the outcome without assuming remote DAG admission. It makes no quorum, finalizer, justification validity, bonded-stake or wire-format change.

## Required verification

1. Rust tests and Clippy including `block_hash_broadcast_retries_only_the_failed_peer` and `block_hash_broadcast_does_not_retry_forever`. Audit format and any transport mock implementing `broadcast`'s result cardinality.
2. Identical source-pinned *canonical* N=8 two-cycle partition/rejoin campaign, unmodified topology/workload, original state volumes and marker, all rejoin finality/heights/hashes, and at least +3 finalized height in 300 s for each cycle.
3. Inspect logs for per-peer transport rejections and retry outcome, and compare accepted DAG sender-tip visibility across peers. If all transport handoffs succeeded but accepted DAG lags remain, the next focus must be block acquisition/validation latency and parent-snapshot formation, **not** weaker quorum or additional blind retries.
4. Only then decide whether this transport correction is separable as its own PR onto `dev`, and whether #407 is necessary/safe. #403 remains a separately attributable experiment.

**Fail closed:** a Rust CI PASS, one cycle PASS, or improved head block height is not release acceptance.

## Follow-up from the failed exact-head Rust gate

[Rust run 39652222932](https://github.com/aixaria0/rchain-rust/actions/runs/39652222932), source `c075d56cd4a3887e13b74bfc576725376da554e1`, stopped in the block-storage library: 63 passed, one failed. The failing new outlier fixture expected collective boundary 61 before sender 0 had advanced beyond its own boundary entry. Its initialization now supplies that missing participation; the outlier sequence starts after it. The sender-local closure rule is retained, and the invariant that one fast sender cannot retire the online quorum is unchanged. This failure prevented the later workspace suites from running; it did not establish a complete Rust pass.

## Bounded announcement pipeline and recipient attribution

The preceding canonical capture `39642261751` also distinguishes reception from admission. For example, bootstrap logged receipt of `88058dd2f7b458c85fa4d70752169c91e36a682741df4e5b0c0693cba3960df0` at 17:25:26.038Z, but its hash broadcast completed at 17:28:47.655Z. Its child `9374afd5c2006b0e4879a913e68cfcd1d9efdeff2a6ba9c39f2530f712788ef0` was received at 17:25:32.386Z yet absent from the bootstrap accepted inventory captured before healing. These observations identify pipeline delay; they do not alone prove which await consumed all that time.

A concrete serial wait exists in `block_processor::apply`: after each validated DAG insertion and completion notification, it awaited a broadcast to all peers before processing the next result. The gRPC send timeout is five seconds, including a disconnected peer on every announcement. The processor now keeps at most 32 hash-only announcement tasks in flight, reaps them, backpressures at capacity and drains on input closure. It preserves validation-before-insertion-before-notification and announces only successfully validated blocks. Tests hold one announcement indefinitely while observing another complete, and exercise capacity backpressure plus shutdown drainage.

A second concrete defect affected retry attribution: gRPC broadcast collected `FuturesUnordered` results in completion order, while `CommUtil` paired them with peers by input index. It now polls concurrently using `FuturesOrdered`, preserving recipient order. A socket regression makes the first peer wait for TLS and a later invalid identity fail immediately, then requires timeout/parse-error results at their original peer indices.

The canonical scripts, 300-second window, two cycles, >=+3 finalized height, marker, original volume, eight-node finalized hash/height equality and full bonded-stake denominator are unchanged. These corrections require a new complete Rust pass and exact-head canonical run. They are not a passing N=8 verdict or universal safety proof.

## Validation of the bounded pipeline and socket-fixture correction

Source `6a19c9b28eedd8d8c4894124450eacbe0b1594cb`, [Rust CI 39654519193](https://github.com/aixaria0/rchain-rust/actions/runs/39654519193), passed the sender-boundary/outlier regressions, both bounded-announcement tests, and both existing finalization tests that had failed before the sender-local correction. Formatting/Clippy and audit also passed. The workspace run then stopped in comm: 142 tests passed, and the new ordering fixture failed because its numeric-only mock identity was rejected before reaching the expected network timeout.

The socket fixture now uses a TLS-valid hexadecimal identity containing letters. The old failed-connection fixture also queried lazy channel construction instead of actual delivery; its assertion now exercises two failed RPCs. The existing mutual-TLS round-trip test is strengthened to fail a real RPC first, start the server, and require successful delivery using the same client/channel within ten seconds. This preserves identity verification and checks recovery rather than treating a channel handle as proof of a connection.

The earlier production source `c075d56cd4a3887e13b74bfc576725376da554e1` [canonical run 39652223071](https://github.com/aixaria0/rchain-rust/actions/runs/39652223071) passed the healthy baseline but failed partition progress (57 -> 59) and timed out healing in 150 seconds. Source `6a19c9b` has a separate running canonical campaign. Workflow concurrency is now keyed by source SHA, so a verification-only follow-up can be tested on its own disposable runner without cancelling or waiting behind an older source. The canonical workload and verdict requirements remain unchanged.

## Two-cycle progress witness and strict rejoin readiness

Source `6a19c9b28eedd8d8c4894124450eacbe0b1594cb`, [canonical run 39654519130](https://github.com/aixaria0/rchain-rust/actions/runs/39654519130), returned `n8=0` and `n8-partition=0`. The first cycle advanced all seven online validators from finalized height 58 to 64 in approximately 131.68 seconds; the second advanced 77 to 83 in approximately 20.23 seconds. Both retained the original state volume and finalized marker snapshots. The independent strict checker nevertheless **failed** cycle 1: its stored rejoin observation had seven validators at height 77/hash `64a3d536657c...`, while the rejoined validator was still at 64/hash `fdc5135e9ed9...`. Cycle 2's eight recorded heads agreed at 83. This is progress evidence, not a full acceptance pass.

The previous healing readiness condition checked only a common minimum height and marker retention. It could return before the returning validator reached the other validators' current finalized identity. Readiness is now strengthened to require all original validators to have the identical finalized height/hash, with the marker check still required, **inside the existing 150-second healing deadline**. The independent post-check remains unchanged. The topology, supporting workload/cadence, +3 requirement, 300-second partition window, two cycles, original volumes/snapshots and bonded denominator remain unchanged.

Five negative/positive controls cover lagging heights, conflicting hashes at equal heights, missing/duplicate validator identities, a complete agreeing set with the marker check, and persistent disagreement failing at 150 seconds. All 15 recovery harness controls pass locally. This follow-up changes the harness readiness condition; it does not change node production code or declare any earlier failed run green.

## Lost block-response handoff during rejoin

Source `07200a2560c0c798119f7eddc893ed7fba59a051` passed [complete workspace CI 39656582259](https://github.com/aixaria0/rchain-rust/actions/runs/39656582259). Its separate [canonical run 39656581938](https://github.com/aixaria0/rchain-rust/actions/runs/39656581938) passed the healthy baseline and advanced all seven online validators 57 -> 60 in approximately 120.98 seconds, then failed the unchanged 150-second healing bound.

The failure capture distinguishes one missing ancestor from replay throughput. The returning validator's accepted inventory stopped near height 61, while the other seven continued above 110. Comparing the accepted inventories leaves one ready frontier block, `71e9dc93154c579bdd1b9d5b228fe443d5ca4e82e29310cf635d5b65040fe1cb` at height 60, whose parents were already accepted on the returning validator. That validator admitted its hash at 19:03:15.650Z. Bootstrap logged the block request and "Response sent" at 19:03:15.651Z, but the returning validator never logged receipt of this hash before the healing deadline. Later payloads were received and waited on that ancestor. "Response sent" does not prove successful streamed handoff.

The concrete stream path discarded both RPC rejection and timeout and had no short retry. Ordinary retriever maintenance defaults to a 240-second age threshold. A stream now makes at most two bounded five-second attempts, separated by 100 ms, reusing the same payload; final failure is reported with peer and packet type. This changes response delivery, not the retriever configuration, quorum or acceptance deadlines.

Three controls use real mutual-TLS stream RPCs: one server refusal followed by exact payload delivery; persistent refusal stopping after exactly two RPCs; and an initial success producing exactly one RPC/delivery. The server's injected refusal is before application dispatch, while accepted streams still use the production certificate/header and reassembly checks. Full workspace and exact-source N=8 validation remain required; the missing-ancestor observation alone does not establish which transport error rejected that payload.

## Renewed demand for a dependency whose earlier reply was lost

[Full CI 39660845018](https://github.com/aixaria0/rchain-rust/actions/runs/39660845018), source `887b68f389fc143c6b404b45b1dea6d56a47320d`, passed all workspace tests and static/format/lint/audit gates. All three real stream-RPC controls passed. This does not supersede the failed rejoin in [same-production-source campaign 39660210172](https://github.com/aixaria0/rchain-rust/actions/runs/39660210172), source `9ff3b308b4d731ee3bc456b1fda7491232f1f9d0`.

That campaign advanced the seven online validators 57 -> 60, but the returning validator's accepted inventory remained at approximately 61 while the others reached 102. Two ready frontier ancestors, `832305b754ae995a83af3a38d06d2dce281904b83e044bf63d723d2b9ddc96fe` and `94b88810b973d0f4a15cd49f6c60d1b06c4ce8d8b61e63a30e19b2d177ec0366`, had been requested during the partition and never acknowledged. The new stream diagnostics recorded final block-response timeouts to the isolated peer. The 150-second healing wait ended before ordinary 240-second request maintenance retried them.

The receiver returned only *unseen* missing parents from its state transition. Parents already marked Requested were excluded, and the retriever ignored a known hash admitted without a new source. Thus fresh child blocks after healing could renew the need for the missing ancestor without renewing its failed request.

The incoming path now submits all not-stored parent hashes, including outstanding ones, to a dedicated missing-dependency request method. Existing young requests remain untouched. Renewed child demand may retry an older unresolved request at most once per 30 seconds, reserving the timestamp under the request lock before network I/O. Known sources are tried in order; exhausted or absent sources trigger discovery. Tracking remains bounded and an actual receipt still removes the request. No parent is released for validation until its required DAG dependencies are accepted.

Controls exercise a lost reply followed by renewed demand, repeated-child suppression, exact block-request identity, retirement after receipt, rotation to another known source and renewed discovery without request-map growth. The ordinary retriever timeout/configuration, node topology, supporting workload/cadence, full bonded denominator and canonical 300/150-second acceptance windows are retained. A complete new-source CI and strict N=8 campaign remain required.
