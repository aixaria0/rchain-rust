# Rust vs. Scala — how the rewrite made the fragile explicit

This page records how porting the RChain node from Scala/JVM (+ the C++ Rosette VM) to Rust
changed *how we reason about* the code's fragile patterns, bugs, and exploits — and why the Rust
node can now **surpass** the Scala original for production readiness, on top of the JVM's garbage
collection and memory problems.

It is the companion to [`AUDIT.md`](AUDIT.md) (the findings check-off) and
[`TYPE-SYSTEM.md`](TYPE-SYSTEM.md) (the type discipline). Where Scala and the specification
disagree, the specification is the oracle; the Scala code is reference material whose latent bugs are
**documented, not reproduced**.

---

## 1. The Scala fragility catalog (concrete, caught in the port)

The Scala node was not merely GC- and heap-bound — it was **notoriously fragile** at the exact
boundaries where correctness matters. The port caught each of these as a *type error* or a *code
review finding*, rather than as a runtime incident:

| # | Scala behavior | Why it is a bug / exploit | How Rust makes it impossible or explicit |
|---|---|---|---|
| 1 | `Costs.toProto` = `PCost(c.value)` — a negative `Long` gas cost wraps into a `uint64` | Over-charging a deploy wraps its cost to a huge unsigned value, corrupting accounting | Negative cost is **rejected** at the boundary (`casper/src/runtime_manager.rs`), not wrapped |
| 2 | Super-majority computed as `stake.toDouble / totalStake > 2d/3` | `f64` loses precision for stakes ≥ 2⁵³; two sides of a fork can disagree on a finality vote | Exact integer `3·stake > 2·total` in `i128` (`sdk/src/consensus.rs`) |
| 3 | `spatial_match_fn(…).ok()?.next()` — a `RholangError` swallowed as "no match" | A Law-5 `BugFoundError` is silently treated as a non-match, corrupting reduction | The error is recorded and propagated; matching is total in `Result` |
| 4 | `getUnsafe` / `.get(...).get` / `unwrap_or(0)` on a negative gas cost | Silent partiality: a missing key or negative value becomes `0`/`None` and the node keeps running on corrupt data | Refinement newtypes (`NonNegI64`, `BlockHeight`, `SeqNum`, `Port`, `WireLen`, `Hash32`) carry the invariant *structurally*; no `Deref`, no public `.0` |
| 5 | `maxMessageSize - 2048` in the chunker | Underflows (wraps) when the max size is small, disabling the size guard | `checked_sub` returns `Err` on a too-small max |
| 6 | Radix-tree node as `Vec[Item]` with `NUM_ITEMS = 256` | The "exactly 256 slots" invariant is implicit; a short/corrupt node panics on indexing | `[Item; 256]` fixed array — the invariant is the type |
| 7 | Exceptions as control flow (`throw`/`catch`, `???`, `NotImplementedError`, `BugFoundError`) | A `???` stub or a `BugFoundError` thrown deep in a `Future` aborts a task silently | `Result`/`Option` everywhere; `todo!()`/`unimplemented!()` are compile-time-visible and gated by the audit script |
| 8 | `HashMap` iteration order feeding `New.injections` / matcher state | Non-deterministic ordering → two nodes reach different state hashes → consensus fork | `BTreeMap`/`BTreeSet` (sorted) and the `Sorted<Par>` refinement (canonical by construction) |

The "fragility" was not incidental: the JVM hid these behind `null`, unchecked casts, boxed
primitives, and catch-all `Try`/`Either` recovery. The Scala node ran *despite* them — the Rust node
refuses to compile *until* they are made explicit.

---

## 2. How Rust's model enables the reasoning

The port does not just *translate* the Scala; it re-expresses each invariant so that the compiler
enforces it:

- **Ownership & the borrow checker** eliminate the aliasing races that made Scala's shared mutable
  `Ref`/`var` state (e.g. the global `connections` write-lock held across I/O, the `BlockRetriever`
  map) hard to audit. Rust's `Mutex`/`RwLock`/`Arc` make *who may mutate what, when* explicit.
- **`enum` + exhaustive `match`** replace null/partial-functions with closed sum types. A
  `NotImplementedError` in Scala becomes a `Result` arm in Rust — the compiler forces you to handle
  the failure case.
- **`Result`/`Option`** replace exceptions and `null`; every partial boundary is a type. The audit
  script (`tools/audit-type-system.sh`) then machine-gates the remaining `unwrap`/`expect`/`panic!`/
  `unsafe`/`assert!` sites.
- **Refinement newtypes** (`NonNegI64`, `BlockHeight`, `SeqNum`, `Port`, `WireLen`, `Hash32`)
  carry domain invariants in the type, so "is this stake negative?" or "is this height
  `-1`?" is not a runtime question — it cannot be represented.
- **`Send`/`Sync`** make concurrency safety a compile-time property, not a code-review convention.
- **Zero `unsafe`** across the crate graph — the entire node is safe Rust, so the class of memory
  bugs Scala/Rosette could hit (use-after-free in the C++ VM, JNI boundary errors) is absent by
  construction.
- **Deterministic collections** (`BTreeMap`/`BTreeSet`, explicit sorts) remove hash-iteration
  non-determinism, which is load-bearing for a consensus node.

---

## 3. Surpassing Scala for production readiness

Beyond the memory-safety argument, the Rust node is *more* production-ready than the Scala one in
concrete, auditable ways:

1. **No GC pauses / no JVM heap blowup.** Scala boxed every `Par`/`Expr` node and every event in the
   hot reduction path; long-running nodes suffered stop-the-world pauses and heap pressure. Rust's
   value semantics and explicit allocation give predictable latency and a small, bounded footprint.
2. **No `Vec::with_capacity(attacker_count)` OOM.** The Scala scodec decoders (and the naive Rust
   port of them) trusted 32/64-bit length prefixes from the wire; a malicious length could allocate
   gigabytes. Rust made these *visible* as `with_capacity`/`try_into` sites, so they could be audited
   and bounded (see `AUDIT.md` C2/C3 and the scodec findings).
3. **The defensive wins are only expressible in a safe language** — semaphore-bounded dispatch,
   per-peer rate limits, a content-addressed Merkle radix tree, mutual-TLS identity pinning, and an
   exact integer consensus — and are now *structural*, not advisory.
4. **The port actively fixed Scala bugs** rather than reproducing them: negative cost, f64
   finality, the swallowed matcher error, the underflowing chunker, the `Vec` radix node, and —
   through this remediation — the unenforced `phlo_limit`, the equivocation/failed-block liveness
   gaps, and the remotely-triggerable panics that were faithful Scala behavior.
5. **The port adds a bound the Scala reference does not have, as a deliberate divergence.** The Scala
   `blockSummary` composes no per-block limit at all: the per-deploy phlo budget bounds each deploy
   *separately*, and nothing bounds the block, so a proposer could make one block arbitrarily expensive
   to replay on every validator. The receiving side now enforces two bounds — a deploy-count cap
   (`validate::deploy_count`, mirroring the proposer's own `MAX_BLOCK_DEPLOYS`, which the proposer had
   always applied to itself and the validator never applied to a peer) and a per-block phlo cap
   (`validate::block_phlo`, summed over the **signed** `phlo_limit` of the block's deploys rather than
   the proposer-supplied `cost` field, which replay recomputes and which a proposer would therefore be
   setting for itself). Both are protocol constants rather than config values: two operators running
   different values would disagree about which blocks are valid, which is a fork — the same reason
   `MAX_BLOCK_DEPLOYS` is a constant. Pre-testnet, so the hard fork costs nothing today.
   Rationale and measurement: [`docs/src/node/security-audit.md`](../docs/src/node/security-audit.md) §3.
6. **The set/map deduplication is ordered rather than scanned.** `par_set`/`par_map` deduplicated with a
   linear scan ahead of an already-Θ(N log N) sort, making every `Set`/`Map` operation Θ(N²) — and
   because a Set operation charges *flat* phlo, a 229 KB deploy bought roughly 98 CPU-seconds for 13
   phlo. Both now sort by `Par`'s total order and dedup the adjacent runs, which is byte-for-byte the
   same output at Θ(N log N). See `models/src/sorter.rs`.
7. **The DAG writes a block's fringe record before its metadata, where the Scala writes metadata
   first.** `BlockDagKeyValueStorage.insert` records the fringe data — the thing a block *refers to* —
   before `block_metadata_store.add`, which is the *pointer*: metadata is what makes a block known, so
   `contains` short-circuits a re-insert and the receiver drops a re-received known block. With the
   Scala's order a crash between the two writes left a block present with no fringe record, and
   nothing rebuilds one — `create`'s fold skips a missing entry in silence while its three sibling arms
   fail closed, and `get_pre_state_for_parents` then refuses every block for which the torn one is the
   max-fringe parent. On a single-validator node that was permanent and silent, recoverable only by
   deleting the shard data dir so `dag_set` empties and `NodeSyncing` runs. Data-then-pointer is the
   order `rspace/src/history/roots_store.rs` already uses; this brings the DAG side into line with it.
   **The Scala's own order is the deviation** (`BlockDagKeyValueStorage.scala:54` vs `:83`), so the
   reorder is the port's, and it is registered here rather than in a findings row. Found by the
   September 2026 audit (F-5), which demonstrated it with four tests over a reconstructed store.

   *Provenance note:* this change is recorded here rather than in its own commit message because a
   concurrent session's `git commit` swept the staged files into an unrelated commit
   (`19259c733`, "Depth.lean's own notes stop saying clause b is owed"). The code and its tests are
   correct in the tree; only the message that should have carried this reasoning was lost, so it is
   written down where the divergence already belongs.
8. **The block `version` is now checked on the acceptance path, where the Scala never checked it
   either.** `validate::version` (`SUPPORTED = [1]`) has existed since the port with a unit test and
   **no production caller**: the acceptance path read the block's hash, signature, shard and deploy
   data, and never the field naming the protocol it is written against. The Scala is the same — its
   `BlockReceiver` carries `// TODO: check valid version` in the same conjunction, and its
   `Validate.version` likewise has no caller — so this is a deliberate divergence rather than a
   fidelity fix: the reference accepts a `version: 999` block, this port now refuses one. It matters
   because the field is inside `hash_block`'s cover, so a future version bump would otherwise have
   been enforced by nothing, and two nodes disagreeing about which versions they accept is exactly
   the divergence the field exists to prevent. Found by the September 2026 audit (F-6).

   **Still open from the same finding:** `timestamp` is likewise inside the hash and read by no rule,
   and it is exposed to contracts on `rho:block:data`. It is not fixed here. A correct bound is a
   policy choice — a `now ± slack` window imports the node's clock into consensus, which this node's
   own audit flags elsewhere, while a monotonic "not before your parents" floor is skew-independent
   but needs a parent-block read the store API does not make obvious. Left for a decision rather than
   guessed at; the audit rated it P3 for the chain as it stands, since no default-genesis contract
   consumes the channel today.

The honest caveat is in §5: the port is not yet *done* surpassing Scala. Several Scala behaviors were
initially carried over faithfully (the "deferred" surface, the panic-vs-exception sites) precisely
because they were faithful — and the remediation plan exists to convert those into Rust-strength
invariants.

---

## 4. What still lags (honest)

- **Formalization**: nothing here any more — law 1b's row reads "**no axioms, from twelve — the
  residual is empty**" (the list comparators' laws by induction on the list, the element laws as
  theorems in dependency order), and `Rchain/Sort.lean` declares no `axiom` at all. This bullet used to
  say "the thirty element-comparator axioms … remain to discharge"; it was stale by the time a reader met it
  and is corrected here rather than deleted, so the record shows what was discharged (AUDIT C72).
- **Native PoS lifecycle**: the dynamic-validator lifecycle is implemented natively — trusted
  stakeholder admission (`trust`/`untrust`), minimum/maximum-bond validation, pool updates with a
  top-N active cap applied **at epoch boundaries**, the epoch reward split and its committed-rewards
  map, staged withdrawals paid out of the staking vault after quarantine, and stake-confiscating
  slashing to the Coop vault (documented in `spec/RUST-FIRST.md`, modelled in `spec/Rchain/Pos.lean`).
  Still deferred: the vault **unforgeable-name capability** (the vault stays a balance map keyed by
  REV address) and the `revvaultexport` tooling.
- **Accepted-faithful residuals** (by design, not defects — see `spec/audit/passes.md` §5/§11): plaintext
  external-IP discovery (M7), the DAG `seen`-cache Θ(N²) *residency* (H6 — its per-clone cost is no
  longer paid: `seen` is shared behind `Arc`, and neither reading nor extending the DAG copies it,
  per the 2026-09-24 pass; the residual *size* is now reported rather than estimated, by the DAG's own
  `logical_bytes` gauge — measured live at **556 MB on the 5,885-block devnet chain, inside a 1.18 GiB
  process** that advances ~1.3 MB per block, where this row's predecessor figure was an estimate from
  Σ|seen| × 32 B on a tree that copied the DAG), and the rate-limited-but-plaintext
  Kademlia discovery bind.

The earlier "deferred/unwired" surface (Kademlia, the HTTP transaction API, block reporting, the
rholang parser's genesis gaps, peer store-items ingress) is now **wired and fixed** (see
`spec/audit/passes.md` §8/§11); the `rho:regex` system process never existed in the Scala oracle (the `regex`
crate was orphaned and has been removed). The audit gate (`tools/audit-type-system.sh`) is **clean** — zero production
`panic`/`unsafe`/silent-conversion, with the remaining `assert!` sites whitelisted as documented
internal invariants; equivocation rejection and finalizer fringe advancement now have regression tests
(`spec/TEST-COVERAGE.md` G1/G8).

---

## 5. Cross-links

- [`AUDIT.md`](AUDIT.md) — the check-off; [`audit/passes.md`](audit/passes.md) — the pass record and the Scala-deviation log (§6).
- [`TYPE-SYSTEM.md`](TYPE-SYSTEM.md) — the ρ→CoC type discipline and refinement types.
- [`RHO-CALCULUS.md`](RHO-CALCULUS.md) — the ρ-calculus grammar, sorts, and operations.
- [`INVENTORY.md`](INVENTORY.md) — the invariant catalog.
