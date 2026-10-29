# Ingress admission: the pool, the permit budget and the concurrency bounds

Three mechanisms sit around the deploy and explore routes, and they answer different questions. The
**deploy pool** is the throttle (Law 86): it bounds how much pending work the node holds, and a full
pool refuses with **429 + `Retry-After`**. The **headroom permit budget** bounds how much executes at
once; memory pressure never refuses admission. The **HTTP in-flight bounds** (from #426) are opt-in
operator concurrency caps and also answer 429.

This page was the #420 admission record. The memory gate it described — a boolean 503 for every
deploy and explore below the throttle floor — was removed by
[#477](https://github.com/rhovision-org/rholo/issues/477): it turned an undersized host into a
network-wide liveness failure while the pool had free slots. The history is kept here; the contract
below is current.

## The pool is the throttle (on)

`casper/src/dag.rs` bounds the pending pool by count, term bytes, per-deployer share and per-deployer
rate. Those are **real bounds** and the only conditions that refuse an otherwise valid deploy:

- **HTTP** `429` + `Retry-After: 1` (or the computed value, once the drain estimate lands) with a
  JSON body naming the bound — pool count, term bytes, deployer share or rate. A malformed request,
  a bad signature, a wrong shard or an expired anchor remains `400`.
- **gRPC** `RESOURCE_EXHAUSTED` with a `retry-after` metadata key on the external deploy service.
- **A duplicated deploy** is refused by the pool's duplicate reservation before the parse; it is not
  a new admission.

`acceptsDeploys` on `/api/status` is the pool's count/byte capacity. `degraded` is the fallback state
machine's **advisory** word; it does not gate admission. A node that cannot execute now holds the
work and, when the relay is enabled, offers it to peers. A deploy with a free slot is accepted under
`normal`, `throttled`, `critical` and `unavailable`.

The term bound (64 KiB) is applied after the body decode, so the decode caps — 16 MiB gRPC / 2 MiB
HTTP — remain above it; that asymmetry is a known residual (H-RS-03/H-RS-05), not the admission
verdict.

## The headroom permit budget (on)

`casper/src/budget.rs` turns the watchdog's headroom into a small permit count:
`headroom = 0 → 0`, otherwise `max(1, min(cap, headroom / 64 MiB))`, capped by
`min(8, available_parallelism)`. Execution takes the floor and exploration the surplus (at most two),
so a node with one permit still makes chain progress. `node/src/resources/budget.rs` holds the
in-flight counters; a request that cannot take a permit is refused **429 + `Retry-After`**, never 503.
The limit follows the five-second snapshot, so a recovering node raises it without a restart, and
in-flight work is never revoked.

Critical/Unavailable still stops block production (the host is in OOM territory); it does not stop
admission or relay. The operator status reports `executionPermits` and `explorePermits`.

## The HTTP concurrency bounds (opt-in)

Both settings are **unset/disabled by default**:

| Variable | Scope | Accepted setting |
| --- | --- | --- |
| `RNODE_HTTP_MAX_INFLIGHT_DEPLOY` | POST `/api/deploy`, `/api/v1/deploy` | integer 1–1024 |
| `RNODE_HTTP_MAX_INFLIGHT_EXPLORE` | All four legacy/v1 `explore-deploy` and `explore-deploy-by-block-hash` routes | integer 1–1024 |

An explicitly malformed, zero or oversized setting fails public HTTP listener startup. Both budgets
are separate. Saturation returns HTTP **429**, JSON `"HTTP in-flight capacity exhausted"` and
`Retry-After: 1`, before body decoding, without queuing. A permit remains held through the downstream
handler future and is released on completion or cancellation.

The served `GET /api/v1/openapi.json` contract covers **all six** POST deploy/explore legacy and v1
aliases, including the `429` response, its JSON error string and the `Retry-After` header. The
`openapi_http_admission_refusal_matches_all_six_live_aliases` regression checks each declared alias
against the actual refused router.

**Example only on a disposable test node (not production sizing):**

~~~sh
RNODE_HTTP_MAX_INFLIGHT_EXPLORE=2 RNODE_HTTP_MAX_INFLIGHT_DEPLOY=4 rnode run ...
~~~

## The pending-work relay (opt-in)

`RNODE_DEPLOY_RELAY=1` offers each admitted deploy to the connected peers' external deploy service
(`RNODE_DEPLOY_RELAY_PORT` overrides the destination port; by default the port is
derived per peer as its advertised protocol port + 1, e.g. 40400 → 40401 and 41400 → 41401;
`RNODE_DEPLOY_RELAY_FANOUT` caps the peers). The receiver
runs the **same** ingress validation as a direct submission and pools the deploy; the sender keeps its
own row. A bounded seen set stops a deploy circulating. The relay is off by default: a network whose
peers do not expose the API port would spend a dial per deploy for nothing. It is the answer to a
constrained node being the only proposer of the work it holds (Law 86e).

## Read-only visibility

`GET /api/v1/admission` publishes only the configured HTTP concurrency budgets, not the pool and not
a node-health verdict:

~~~json
{"deploy":{"configured":true,"limit":4,"inFlight":1,"available":3},
 "explore":{"configured":true,"limit":2,"inFlight":2,"available":0}}
~~~

When a budget is disabled, it is `{"configured":false}`. The sampled headroom, floors, recovery
boundary, permit limits and sample age are on the operator status (`/api/v1/status/operator`), which
is the surface that carries numbers. A snapshot is not a memory budget, network readiness, finalised
height, or validator safety guarantee.

## Acceptance and rollback

1. `cargo fmt --all -- --check`; `cargo test -p rchain-node --lib` (the pool, permits, 429 matrix,
   relay wiring); `cargo test -p rchain-casper --lib batch::` and the two
   `runtime_manager::tests::a_*_batch_*` equivalence tests. The negative cases exercise a full pool,
   a refused share/rate, a malformed body, every legacy/v1 alias before JSON decode, 429 +
   `Retry-After`, independent budgets, permit release, and status access under pressure.
2. In an **isolated disposable environment** only, compare the same binary/workload with a
   constrained cgroup versus a healthy host, recording tail latency, 429 rates, memory high
   watermarks, RPC and finality progress, and post-cancellation permit recovery. No public-testnet
   stress.
3. Choose concurrency limits from measured per-node/cgroup capacity. Do not reuse the old
   0.25 MB/block replay estimate as a steady-state number, assume two validators on one 1 GB host are
   independent, or interpret low `MemAvailable` as proof of kernel OOM.
4. Rollback: there is no memory-gate switch to restore — it was removed. The HTTP concurrency budgets
   roll back by removing their two environment variables; the relay rolls back with
   `RNODE_DEPLOY_RELAY=0`; the batch path is off unless `RNODE_CROSS_DEPLOY_BATCHING=1`. Nothing about
   genesis, chain state, protocol messages, hash validity or fork choice changes.

## Remaining #420 gates

- **Start-up/cgroup preflight:** unit A2 (chain-length floor, validators enforce / observers warn,
  `--allow-under-minimum`), reviewed separately.
- **Measured memory backpressure:** the term-byte pool bound is a proxy, not measured peak Rholang and
  merge resources; the replay/merge peak is H-RS-04.
- **Fringe readiness and bootstrap failover:** versioned negative-response semantics, logs on
  sender/receiver, safe anchored-root selection, backward-compatibility and two-peer recovery tests. An
  unavailable anchor must never silently fall back to genesis.
- **Finality/peer degraded signal:** requires cross-node timed observations of finalised
  height/hash/state and a defined alert/readiness policy.
- **Root write integrity:** needs the raw B/C JSON at pinned hashes, exact installed Root contract
  revision, accepted/rejected deploy-chain and finalised post-state provenance, and disposable replay.
  Tools in #421 do not prove a fix.

**No validator, live testnet, consensus, or deployment changes are authorized by this diagnostic
improvement.**
