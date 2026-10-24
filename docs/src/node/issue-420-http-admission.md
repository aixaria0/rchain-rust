# Ingress admission: the memory gate and the concurrency budgets

Two independent gates sit in front of the deploy and explore routes, and they answer different
questions. The **memory gate** (unit B34 of the resource-safety programme) is always on: a node below
its admission floor refuses work before it decodes the body. The **concurrency budgets** (from #426)
are opt-in: an operator who has measured a per-node limit can cap how many deploy/explore requests run
at once. Neither is a fix for consensus liveness, fringe recovery or loss of finalised Root writes.

## The memory gate (on)

The gate reads the resource watchdog's newest sample — `MemAvailable` capped by the cgroup's remaining
allowance — and refuses a deploy or explore when:

- available memory is below `max(256 MiB, 10% of the effective limit)`, where the effective limit is
  the cgroup's `memory.max` when set and the host's total otherwise;
- the sample is older than 30 s (six missed watchdog samples); or
- the facts could not be read at all.

A stale or unreadable sample is a **refusal**, never a pass: a stopped watchdog must not leave
admission open on a reading nobody refreshed. The thresholds are a documented policy floor
(`node/src/resources/admission.rs`), not a measured replay curve; the 10 % headroom is the room left
for work already in flight, and the 256 MiB absolute floor dominates on a host around 1 GiB.

- **HTTP**: `503` with `Retry-After: 1` and a JSON body naming the reason, on all six POST deploy and
  explore aliases, **before the JSON body is decoded**. The per-peer rate limit still returns `429`
  independently and runs outside this gate.
- **gRPC**: `RESOURCE_EXHAUSTED` with the same reason on the external deploy service, before the
  handler parses or pools the deploy. Tonic's interceptor runs after the request frame is decoded; the
  pre-decode bound on that surface is `api-server.grpc-max-recv-message-size`, and this gate follows
  it and precedes the work.

`/api/status` (and the operator status) stay reachable while admission is shed, so the operator can
see the node that is refusing work. The gate does not change the public `degraded` word or stop
proposing: those are the fallback state machine's job (unit C).

## The concurrency budgets (opt-in)

Both settings are **unset/disabled by default**:

| Variable | Scope | Accepted setting |
| --- | --- | --- |
| `RNODE_HTTP_MAX_INFLIGHT_DEPLOY` | POST `/api/deploy`, `/api/v1/deploy` | integer 1–1024 |
| `RNODE_HTTP_MAX_INFLIGHT_EXPLORE` | All four legacy/v1 `explore-deploy` and `explore-deploy-by-block-hash` routes | integer 1–1024 |

An explicitly malformed, zero or oversized setting fails public HTTP listener startup. No limit is
guessed without measured host capacity; an operator must configure and validate it in an isolated
environment. Both budgets are separate. Saturation returns HTTP **503**, JSON `"HTTP in-flight
capacity exhausted"` and `Retry-After: 1` before body decoding, without queuing. A permit remains held
through the downstream handler future and is released on completion or cancellation.

The served `GET /api/v1/openapi.json` contract covers **all six** POST deploy/explore legacy and v1
aliases, including the `503` response, its JSON error string and the `Retry-After` header. The document
uses a `/api/v1` default server; the three admission-controlled operations additionally declare the
`/api` legacy server. The `openapi_http_admission_refusal_matches_all_six_live_aliases` regression
checks each declared alias against the actual refused router.

**Example only on a disposable test node (not production sizing):**

~~~sh
RNODE_HTTP_MAX_INFLIGHT_EXPLORE=2 RNODE_HTTP_MAX_INFLIGHT_DEPLOY=4 rnode run ...
~~~

## Read-only visibility

`GET /api/v1/admission` publishes only the configured HTTP concurrency budgets, not the memory gate's
state and not a node-health verdict:

~~~json
{"deploy":{"configured":true,"limit":4,"inFlight":1,"available":3},
 "explore":{"configured":true,"limit":2,"inFlight":2,"available":0}}
~~~

When a budget is disabled, it is `{"configured":false}`. The route remains available even if
deploy/explore slots are exhausted. The sampled memory headroom, its source and its age are on the
operator status (`/api/status/operator`), which is the surface that carries numbers. A snapshot is not
a memory budget, network readiness, finalised height, or validator safety guarantee.

## Acceptance and rollback

1. Run `cargo fmt --all -- --check` and `cargo test -p rchain-node --lib admission`, plus
   `cargo test -p rchain-node --lib http_inflight` for the concurrency budgets. Negative cases exercise
   malformed settings, a pressured/stale/unreadable sample, every legacy/v1 alias before JSON decode,
   503 + `Retry-After`, independent budgets, permit release, and status access.
2. In an **isolated disposable environment** only, compare the same binary/workload with the gate
   exercised (a constrained cgroup) versus a healthy host, recording tail latency, 503/429 rates,
   memory high watermarks, RPC and finality progress, and post-cancellation permit recovery. No
   public-testnet stress.
3. Choose concurrency limits from measured per-node/cgroup capacity. Do not reuse the old
   0.25 MB/block replay estimate, assume two validators on one 1 GB host are independent, or interpret
   low `MemAvailable` as proof of kernel OOM.
4. Rollback: **no switch disables the memory gate** — a node that cannot read its own headroom is
   exactly the case the programme exists for; to run one anyway, raise the cgroup limit or stop the
   node. The concurrency budgets roll back by removing their two environment variables. Nothing about
   genesis, chain state, protocol messages, hash validity or fork choice changes.

## Remaining #420 gates

- **Start-up/cgroup preflight:** unit A2 (chain-length floor, validators enforce / observers warn,
  `--allow-under-minimum`), reviewed separately.
- **Measured memory backpressure:** the term-byte pool bound is a proxy, not measured peak Rholang and
  merge resources; complete queue accounting is owed.
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
