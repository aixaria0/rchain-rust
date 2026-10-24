# #420 — HTTP in-flight admission (opt-in)

This is a *bounded HTTP concurrency* mitigation, **not** a fix for host OOM, consensus liveness, fringe recovery, or loss of finalized Root writes.

## Configuration

Both settings are **unset/disabled by default**:

| Variable | Scope | Accepted setting |
| --- | --- | --- |
| `RNODE_HTTP_MAX_INFLIGHT_DEPLOY` | POST `/api/deploy`, `/api/v1/deploy` | integer 1–1024 |
| `RNODE_HTTP_MAX_INFLIGHT_EXPLORE` | All four legacy/v1 `explore-deploy` and `explore-deploy-by-block-hash` routes | integer 1–1024 |

An explicitly malformed, zero or oversized setting fails public HTTP listener startup. No limit is guessed without measured host capacity; an operator must configure and validate it in an isolated environment. Both budgets are separate. Saturation returns HTTP **503**, JSON `"HTTP in-flight capacity exhausted"` and `Retry-After: 1` before body decoding, without queuing. Existing per-peer ingress rate limits still return **429** independently. A permit remains held through the downstream handler future and is released on completion or cancellation.

The served `GET /api/v1/openapi.json` contract explicitly covers **all six** POST deploy/explore legacy and v1 aliases, including the opt-in HTTP `503` response, its JSON error string and the `Retry-After` header. The document uses a `/api/v1` default server; the three admission-controlled operations additionally declare the `/api` legacy server. The `openapi_http_admission_refusal_matches_all_six_live_aliases` regression checks each declared alias against the actual saturated in-process router. This does **not** extend admission control to gRPC.

**Example only on a disposable test node (not production sizing):**

~~~sh
RNODE_HTTP_MAX_INFLIGHT_EXPLORE=2 RNODE_HTTP_MAX_INFLIGHT_DEPLOY=4 rnode run ...
~~~

## Read-only visibility

`GET /api/v1/admission` publishes only the configured HTTP budget, not a node-health verdict:

~~~json
{"deploy":{"configured":true,"limit":4,"inFlight":1,"available":3},
 "explore":{"configured":true,"limit":2,"inFlight":2,"available":0}}
~~~

When disabled, each budget is `{"configured":false}`. The route remains available even if deploy/explore slots are exhausted. A snapshot is not a memory budget, network readiness, finalized height, or validator safety guarantee.

## Acceptance and rollback

1. Run `cargo fmt --all -- --check` and `cargo test -p rchain-node --lib http_inflight` plus `cargo test -p rchain-node --lib admission`. Negative cases exercise malformed settings, exhausted legacy/v1 aliases before JSON decode, 503 + Retry-After, independent budgets, release, and status access.
2. In an **isolated disposable environment** only, compare the same binary/workload with admission off versus on, recording tail latency, 503/429 rates, memory high watermarks, RPC and finality progress, and post-cancellation permit recovery. No public-testnet stress.
3. Choose limits from measured per-node/cgroup capacity. Do not reuse the old 0.25 MB/block replay estimate, assume two validators on one 1 GB host are independent, or interpret low `MemAvailable` as proof of kernel OOM.
4. Rollback: remove the two environment variables and use the operator-approved restart procedure. Nothing about genesis, chain state, protocol messages, hash validity or fork choice changes.

## Remaining #420 gates

- **Startup/cgroup preflight:** design needs reliable host + cgroup v1/v2 accounting on co-located validator hosts and a fail/override policy. Arbitrary fixed RAM thresholds risk false safety/rejections.
- **Actual memory backpressure / term-byte bounds:** measured peak Rholang and merge resources, not a term-length proxy, plus complete queue accounting.
- **Fringe readiness and bootstrap failover:** versioned negative-response semantics, logs on sender/receiver, safe anchored-root selection, backward-compatibility and two-peer recovery tests. An unavailable anchor must never silently fall back to genesis.
- **Finality/peer degraded signal:** requires cross-node timed observations of finalized height/hash/state and a defined alert/readiness policy.
- **Root write integrity:** needs Patrick's raw B/C JSON at pinned hashes, exact installed Root contract revision, accepted/rejected deploy-chain and finalized post-state provenance, and disposable replay. Tools in #421 do not prove a fix.

**No validator, live testnet, consensus, or deployment changes are authorized by this diagnostic improvement.**
