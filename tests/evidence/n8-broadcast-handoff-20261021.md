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
