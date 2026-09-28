# Security audit (September 2026)

This chapter is a point-in-time report. It records what an adversarial review of the node found at
commit `67dd6fb7b`, what it could not settle, and how the node's safety properties compare with three
other production chains read from their own source.

It is written to be useful when it is unflattering. The node's one confirmed high-severity weakness is
in this chapter, together with the structural properties that make whole classes of defect
unrepresentable. Both halves matter: a review that lists only residual defects misrepresents a system
whose thesis is carrying invariants in the semantics rather than in review.

**Scope.** Report-only. No code was changed as a result of this pass, and no finding in it has been
fixed or closed. Read it as a photograph, not as a status.

## 1. Method

The approach is the one this project's own audit history found to work, with its refutation stage
rebuilt.

**Three stages.** Independent lenses read the tree adversarially, each required to produce either a
reproduction or nothing. Every candidate finding was then handed to a *second* agent whose instruction
was to refute it. Survivors were graded and given a falsifier.

**Why the refutation stage was rebuilt.** The earlier rule was "default to REFUTED whenever the
mechanism could not be re-established from source". That rule cannot distinguish *the mechanism is
absent* from *the mechanism is real but not re-establishable by reading* — and the second category is
exactly the high-severity dynamic class (races, crafted-depth exhaustion, load-driven denial of
service, crash consistency). A default-REFUTED filter therefore suppresses the best material.

This pass used three verdicts instead — **`CONFIRMED` / `NOT-RE-ESTABLISHED` / `REFUTED`** — where only
`REFUTED` removes a finding, and a `REFUTED` verdict must carry a positive artifact: a named line, a
test, or a measurement showing the mechanism absent. An empty-handed refutation is a self-reported
clean, and this project's audit record already holds that "an independent read is evidence and a
self-reported clean is not".

**The discipline earned its keep.** Ten candidate findings were refuted, including the one the pass
opened expecting to lead with:

| Candidate | Verdict | The artifact that killed it |
|---|---|---|
| Deeply-nested protobuf reaches an unbounded recursion in `wire.rs` and exhausts the stack from the unauthenticated deploy port | REFUTED | `prost`'s `RECURSION_LIMIT = 100` refuses the message at decode, before any handler runs; the server stack is 32 MiB (`node/src/main.rs`) |
| The `qucalc` `i64` overflow panics the interpreter and is a remote denial of service | REFUTED | Continuation dispatch is spawned and `join_spawned` maps the `JoinError`; the deploy fails, the node survives (measured) |
| The Docker image builds an unpinned dependency set with no CI signal | REFUTED | `ci.yml` runs `cargo clippy --locked --workspace --all-targets --all-features` on every push |
| The T1 coverage headline is contradicted by the coverage register | REFUTED | The cited rows are a date-stamped batch snapshot the same document supersedes |
| Vendored genesis `.rho` integrity is checked only at audit time | half REFUTED | The check runs on every push, and an edited contract changes the genesis post-state hash |

Two of the pass's own severity assessments were also corrected by measurement rather than argument: the
`qucalc` `trust_levels` function is **linear**, not quadratic (its relaxation is monotone over levels in
`{0,1,2,3,4,5}`, so it converges in at most six iterations whatever the input size), and the LMDB
crash window is bounded to two fsyncs and can self-heal on a multi-validator network.

**Limits, stated plainly.** The pass read code and ran targeted tests. It did not run a live multi-node
attack, did not re-measure coverage, and did not re-read all 89 T1 modules line by line. Findings below
marked *measured* carry a command and a number; those marked *read* are code-path arguments.

## 2. Results

| | |
|---|---|
| Confirmed P0 (chain split / fund loss / RCE) | **none** |
| Confirmed P1 (remote unauthenticated denial of service) | 3, sharing one root cause |
| Confirmed P2 | 4 |
| Confirmed P3 | 14 |
| Not re-established (refutation incomplete) | 1 |
| Refuted during the pass | 10 |

The absence of a P0 is the headline result, and it is a result about the project's prior work: on a tree
where earlier passes had confirmed remote denial-of-service defects, this pass could not find a chain
split, a fund loss, or remote code execution.

## 3. The gas model does not bound work

This is the pass's principal finding and the only one with a high severity.

**The cost model charges a flat rate for work that is superlinear in attacker input.** Deducing a
`Set` is implemented with a linear scan in `models/src/sorter.rs` (`par_set`), so `Set.union` costs
Θ(N²) — measured with an empirical exponent of **2.43**, i.e. super-quadratic, because the accumulating
result vector is itself rescanned. The cost table charges a **flat 13 phlo** for the operation
regardless of N.

Measured in release, on a term parsed from source:

| N | source | phlo charged | union | total |
|---|---|---|---|---|
| 5 000 | 23.9 KB | 13 | 0.42 s | 0.45 s |
| 10 000 | 48.9 KB | 13 | 1.65 s | 1.7 s |
| 20 000 | 108.9 KB | 13 | 7.97 s | 8.0 s |
| 40 000 | 228.9 KB | 13 | 64.1 s | **98.0 s** |

That is roughly 7.5×10⁹ nanoseconds of node CPU per phlo, against a cost table whose own fair rate is
about one phlo per byte of real work.

**This is a divergence from the Scala reference, not inherited behaviour.** The Scala deduplicates
through a `HashSet` (`SortedParHashSet.apply`), expected O(N). The port replaced a hash set with a
linear scan. `par_map` has the same defect.

Three aggravating factors:

1. **Execution-time parsing is uncharged.** `Costs::parsing_cost` is defined and called nowhere in the
   tree, and `phlo_limit = 0` is accepted at ingress, so a deploy can force the full parse and
   normalise before the first charge lands. The Scala charges `parsingCost` before `sourceToADT`.
2. **Nothing bounds a block.** The validator-side check list has no total-phlo cap and no deploy-count
   cap. `MAX_BLOCK_DEPLOYS` is used only by the local proposer; nothing on the receiving side enforces
   it, and there is no bonded-sender check on the block path. The only transmission bound is the 256 MiB
   streamed block size.
3. **A running call cannot be interrupted.** The reduce-step budget and the cancellation flag are
   checked at continuation boundaries only, so a single builtin call of arbitrarily long duration runs
   to completion. There is no wall-clock timeout on the block execution path.

**Why no gate caught this.** None of the project's gates is about cost. The 50 laws cover semantics —
canonicalisation, substitution, COMM, matching, merge, finality arithmetic. A cost regression is not a
panic, not an `unsafe`, not a type escape, not a coverage drop, and not a law violation, so it is
invisible to every check the project runs. That is a gap in the shape of the register, not in the
strictness of any particular gate.

**Prioritised remediation.** Add a per-block phlo cap and a validator-side deploy-count cap; replace the
linear scan with a `HashSet` (semantically identical — the derived `Hash`/`Eq` are consistent); wire
`parsing_cost` at execution time before the cost is sampled; and check the cancellation flag inside long
builtins. The `par_set` change is two lines and removes the amplification's root.

## 4. Other confirmed findings

**The DAG can wedge silently after a crash (P2).** `casper/src/dag.rs::insert` writes the block
metadata — which marks the block known, so a re-offer is a no-op — *before* the fringe record. A crash
between the two leaves a store where the block is present and its fringe record is not;
`get_pre_state_for_parents` then refuses every block for which that one is the max-fringe parent, and
the missing record is skipped **silently** on restore, while the three sibling arms all fail closed.
The record is keyed by the fringe rather than the block, so on a multi-validator network a later block
declaring the same fringe repairs it. **On a single-validator or standalone node it does not: the node
is wedged permanently and the recovery is to wipe the data directory and resync.** The write order is a
faithful port of the Scala; the fix — write the fringe record before the metadata — is the same
data-then-pointer order the RSpace history layer already follows, and it turns the window into a
harmless orphan record. Making the silent restore arm loud is a complementary one-line change.

**Governance arithmetic is unchecked in release (P2).** `qucalc`'s `rho:gov:*` handlers perform plain
`i64` arithmetic on values taken straight from deploy arguments with no range check. In debug builds
this panics and the deploy fails; in release it wraps, and the wrapped results are silently wrong —
`resolveWeights` returns a clamped zero where a maximum was asked for, a delegation can produce a
*negative* weight, `censure` can promote a voucher from `i64::MIN` to `i64::MAX`, and a ranked tally can
elect the landslide loser or report no winner for a unanimous vote. These are pure functions and every
node wraps identically, so this is a correctness defect for anything reading the governance channels as
an oracle rather than a consensus split.

**Block `timestamp` is hashed and consumed but never validated (P2 for any contract that reads it).**
The acceptance predicates check every other field that the content hash covers, but not `timestamp` and
not `version` (whose predicate exists and has no production caller). `timestamp` is not merely
informational: it is exposed to contracts on `rho:block:data`, so a bonded proposer can choose a value
that every honest validator replays, and a contract that reads it changes its output and therefore the
post-state hash. The `version` half is inherited from the Scala; the `timestamp` half is the port's own.
The generalisable question — *which fields are in the hash but absent from the acceptance predicate?* —
has a four-item answer: `version`, `timestamp`, `rejected_blocks`, `rejected_senders`.

**DAG memory grows quadratically in block count (P2, registered).** Each message retains its whole
ancestry, so total residency is N(N+1)/2 where N is every block ever accepted, and the message map is
never pruned. This is a known, measured residual — the node's own metric reads 17 319 555 entries at
5 885 blocks, exactly N(N+1)/2. What this pass adds is the reachability analysis: the input rate is
attacker-controllable, and nothing a bonded validator must respect bounds it, which puts 16 GB of
resident memory roughly twelve hours away at a two-second block interval.

**Fourteen lower-severity items** span: two production `await-holding-lock` sites that contradict the
audit record's claim that only test modules remain; a published API-schema rule that states the wrong
tag casing for the deploy-status envelope (the wire emits capitalized tags, and the machine-checked
envelope law agrees with the wire); a served `openapi.json` that declares the wrong request body for
`/explore-deploy`; a `cargo-deny` invocation that runs only the advisory check, leaving the authored
licence allow-list as dead letter; no checksum, signature, SBOM or reproducible-build step in any
workflow; a `next-audit-number` helper that does not see an allocated number and would re-issue it;
counted-but-unenforced classes in the type-system gate; a coverage-ledger check that no workflow
invokes; and hand-repeated counts that have drifted (the README's line count and the two documents'
disagreement over how many crates the workspace has).

## 5. What a green register does not cover

The project's check-off is genuinely green — 201 of 201 findings closed, all 89 T1 rows with a verdict —
and, unusually, the law register does not overclaim: all 87 registered Rust witnesses resolve to real
functions, there are **zero `#[ignore]`d tests** in the tree, and the Lean gate refuses `sorry`,
`admit` and `opaque`. That was checked adversarially rather than assumed.

What the register does not cover is specific:

- **The assurance floor moved down and nothing measures the new one.** The close-out deleted the
  instrument, mutation and register-join harnesses on the grounds that their failures were always "a
  document disagrees with the tree". That is a defensible trade, but no mutation tool remains, so the
  real strength of the 87% line-coverage floor is now unmeasured. In the sampled module the coverage is
  assertion-dense (five mutation-style defects, five caught); the aggregate is *execution* coverage and
  is satisfiable by tests that assert only well-formedness. **This pass could not put a single number on
  the workspace's mutation score, and the apparatus that would produce one was deleted.**
- **The computed gates are weaker than they read.** The type-system gate's counted classes — `cast`,
  `lax`, `get`, `index`, `div`, `overflow` — are reported and **not enforced**; the ratchet that once
  failed the build is no longer wired.
- **The push path is much thinner than the nightly.** The formal gate, the coverage floor and the devnet
  fuzz are nightly or manual only, so a spec-side change that breaks a conformance corpus can land on a
  push and stay green for a day.
- **Nothing in the register asks what an attacker pays.** Cost is not a row type the audit can express.

## 6. Inherent and extrinsic safety

Bug-hunting audits the *extrinsic* half of a security posture — what is currently broken, which bounds
fail, which checks are missing. That half must be redone forever, because a check can be bypassed,
removed or forgotten. The other half, in Mark Miller's distinction, is **inherent safety**: a property
that follows from the semantics themselves, holding without trusting the implementer and without a
runtime check continuously enforcing it.

**Two classes of privilege-escalation defect are unrepresentable in rholang, and one is proved.**

*Authority cannot be forged.* There is no grammar production that writes a private name, and no rholang
operation destructures one. A name is allocated from a splittable hash-derived RNG and exists only to be
received: "invoke on a channel you were not given" has no term in the language.

*Authority cannot be captured across a call boundary.* A COMM step transfers exactly the evaluated
datum into the tuple space and substitutes it into the *receiver's* body in the receiver's own
environment; the sender's environment is never consulted. Reaching into a caller's variables or
continuation is not guarded — it is unsayable. Closedness is a **theorem**, not a convention: a closed
program cannot *grow* a free variable by reduction. Confused-deputy, in the sense of a deputy acting
with borrowed authority, has no expression.

Also proved: substitution preserves sort and closedness, sort is functional and decidable, and
canonicalisation is idempotent and commutative. Determinism here is itself a safety property — merge is
a commutative monoid and canonicalisation is idempotent, so execution *order* cannot change the state,
which removes a class of divergence rather than testing for it.

**Honest caveats.** Three, and they matter:

1. **Unforgeability is axiomatised, not proved.** The claim that a name's bytes cannot be guessed rests
   on the crypto axioms (law 19), not on a theorem. A break in the hash would falsify an axiom, not
   contradict a proof. Say "axiomatised at the crypto boundary" — never "proved unforgeable".
2. **The `rho:*` namespace is ambient authority.** Twenty-seven system urns are resolved
   unconditionally for every deploy, so any contract that can spell the public string can reach the
   channel — including `rho:gov:*`, whose arithmetic is unchecked (§4). Mutation methods on some
   channels are additionally gated by the caller's own deployer identity, which is genuine capability
   discipline, but the channel and everything read-only are ambient.
3. **The blessed genesis keys are published constants in the source.** The capability they confer is
   therefore forgeable, and what actually prevents their use is a negative allow-list that rejects
   deploys signed with them. That is the cleanest seam in the codebase: a forgeable capability restored
   by an access-control list.

Genuine capability distribution does exist — a deploy's own identity is bound at normalise time from its
verified signature, so a deploy receives exactly its own identity and never anyone else's.

**The honest summary:** the language and calculus are safe-by-structure and partly proved; the chain
layer's authority distribution is access control by another name; and the cryptographic substrate is
assumed rather than established. That is where assurance ends.

## 7. Comparison with other chains

**Context first, because it is the most relevant fact in any comparison.** Solana, Sui and Bitcoin SV
are live mainnet networks and have been for years; as of September 2026 their market capitalisations
are on the order of $70bn, $5bn and $0.4bn respectively. This node is **pre-testnet**. It has had none
of the adversarial exposure, external audit budget or years of production hardening those networks
have absorbed. A comparison that omits that asymmetry is not neutral — it flatters the incumbents by
grading a codebase that has never faced an attacker as though it had.

With that stated, the comparison is worth making, and it is not made by reading declared limits. Four
nodes were probed with the *same* question, because reading constants and calling them bounds is how a
comparison goes wrong. The question was: **is there an operation reachable by an unauthenticated
remote attacker whose cost to the node is superlinear in attacker input, while the charge to the
attacker is flat or sublinear?**

Every one of the four has such an operation. This is a defect class in the design of VM cost models
generally, not a distinguishing weakness of any one chain.

| | Worst instance found | What the attacker pays | Blast-radius containment |
|---|---|---|---|
| **This node** | `Set` dedup Θ(N^2.43) for a flat 13 phlo; 98 CPU-seconds at N = 40 000. Plus a zero-phlo path | 13 phlo | **no per-block cap**; no deploy-count cap on validators; no block-path timeout |
| **Solana** | `sol_big_mod_exp` charges 8 042 compute units for ≥10 ms of bignum work (≥40×). Every transaction copies up to 64 MiB of account data with **zero units charged** | flat 5 000 lamports, *identical to a transaction that loads nothing* | good: 48 M block compute-unit budget, 64 MiB loaded-data cap |
| **Sui** | A zkLogin native charges a flat 200 units for hashing up to 256 KiB passed by reference and reusable across calls (~82× against its own keccak price) | gas; transactions bounded to 128 KiB | best of the four: instruction tiers rising to 1000×, 128 KiB transaction bound, per-package verification meter |
| **Bitcoin SV** | `OP_CAT` doubles the top stack element with its size guard inside a pre-Genesis branch: **~40 bytes of script → roughly a terabyte of work**. The legacy sighash is Θ(N²) and reachable via an attacker-chosen flag bit, computed *before* verification | byte-shaped fees on a *failing* transaction collect nothing | **none in consensus**: opcode limit `UINT32_MAX`, stack cap `INT64_MAX`, and the cancellation token is skipped on the block path |

**Where this node sits:** mid-pack on amplification — better than Bitcoin SV, comparable to Solana,
behind Sui — and second-worst on containment, because Sui and Solana both bound the blast radius and
this node bounds a block hardly at all. Its instance is also the narrowest of the four to repair: a
two-line regression against its own Scala oracle.

On the inherent-safety axis (§6) the ordering is different, and more favourable:

| | Unforgeable authority | No ambient authority | Encapsulation | Reentrancy / confused deputy | Machine-checked |
|---|---|---|---|---|---|
| **This node** | **unrepresentable** (soundness axiomatised) | none in the language; **27 ambient urns** in the chain layer | n/a — capabilities are copyable references by design | **unrepresentable** | **Lean 4 + Coq; closedness proved** |
| **Sui** | **unrepresentable** — linear `UID` freshly minted from the transaction digest | banned at the bytecode level | **unrepresentable** — linear resources | **unrepresentable** — no dynamic dispatch | static verifier yes; **Move Prover absent** |
| **Solana** | checked — a program-derived address has no private key, but granting the privilege is a runtime check | present — any program may invoke any executable program | absent — accounts are flat cloneable buffers | checked at runtime | ABI digests only, which are checksums |
| **Bitcoin SV** | absent as a concept | fewest surfaces, by having almost no semantics | linear by consensus validation | moot — no calls exist | **nothing** |

Bitcoin SV deserves the specific note that it *negates* the model deliberately: an operator RPC injects
a consensus blacklist, and spending a blacklisted output causes blocks on the active chain to be
disconnected. That is safety by legal process rather than by semantic property — the opposite of what an
object-capability design is for. Sui is the closest analogue to this node and makes a broader set of
properties unrepresentable, but enforces them with a static verifier at publish time rather than in the
semantics, and its prover does not ship.

**Caveats that limit this table.** Solana's VM and bignum library are unvendored registry pins, and
Sui's crypto and P2P layers are git dependencies, so in both cases the component the worst native calls
into — and, for Sui, the entire network ingress path — could not be read. That is recorded as "could not
trace", not as a clean bill of health. The Solana tree compared is the archived `solana-labs/solana`
monorepo, whose last commit predates this audit; the live lineage is Anza's Agave fork.

## 8. Open questions

**Active-validator-set selection has no oracle.** The set is chosen as the top N by descending stake
with a key-ascending tie-break. The Scala contract returns the first N in map-key order and carries a
TODO saying the real rule should be random selection once on-chain randomness exists. The law register
pins the epoch-boundary *timing*, not the membership. If the rule is wrong, the epoch-boundary validator
set differs from any other implementation — a chain split — with nothing in either oracle able to
arbitrate. **This is the highest-consequence unverified item in the tree.**

**Two permissive defaults define what the contract treats as an arithmetic fault.** An epoch length of
zero and a minimum bond of zero make every block an epoch boundary and zero the reward, where the
contract divides by both and faults. A node running these defaults mints and pays on a schedule the
contract cannot express.

**Whether the margin on term depth is worth stating.** The parser and storage depth guards have
measured values but their agreement with the depth measure is an acknowledged proof debt. They are also
a hard fork: a term an older node accepts, a newer one refuses.

**The `legacy/` tree is unmanaged.** It is built and scanned by no CI job, and it carries its own
dependency manifest from 2020–21 whose advisories `cargo-deny` cannot see because it walks only the Rust
lockfile. It is not a runtime surface, but re-enabling its build would resolve to known-vulnerable
versions with nothing to say so.

**Whether the fringe's liveness predicate should be re-decided.** It compares cardinalities — how many
messages, not which senders — so two messages from one sender plus one from a second satisfy a
three-validator bond map. This is a faithful port of an upstream gap and is already registered as an
upstream design property; it is listed here only because a liveness rule that counts rather than names
is the kind of thing worth re-deciding deliberately rather than inheriting.
