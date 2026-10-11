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
