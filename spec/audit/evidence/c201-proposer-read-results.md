# The proposer's pre-state does not advance (C201, #150)

C201's row asked for a live-run measurement once its fixture exonerated the merge. Three runs and two unit
tests later, it is resolved — and the answer is a **measured mechanism**, not a hypothesis.

`spec/audit/evidence/c201/` holds the raw output of all three runs (`proposer-read-run{1,2,3}.txt`, plus
run 1 paired with the proposer's own `block #N` lines).

## The instrument

In the proposer's fold, beside the `bonded` filter: the pre-state it asked, how many validators that
answer held, how many equivocations the node recorded and which the filter admitted, and — added run by
run, as each candidate was eliminated — the parent set, the merge's **fringe state**, and how many deploy
ids the merge **rejected**. Logged only when an equivocation has been recorded.

## The eliminations

Three, each now pinned by a test or a measurement that did not exist before:

1. **The native fold is correct.** C201's owed fixture passes: a slashing branch's native write reaches a
   merged root (`casper/tests/block_index.rs`).
2. **The fringe is not pinned by a stale carrier.** `latest_fringe` follows what a parent *carries*, not
   how tall it is, so a fresh carrier always beats a stale one — a single stale parent cannot pin the
   merge. That test is red under the mutation that reads the parents' heights instead, while the
   pre-existing `latest_fringe_picks_max_height` stays green.
3. **The scope is not pinned by a stalled fringe.** `from_dag`'s base half is fringe-derived, but the
   **conflict scope** is `merge_fringe.seen \ final_fringe.seen`, so an advancing parent strictly grows it
   even with the fringe held fixed.

So neither half of the derivation can produce a constant result across parent sets where one parent
advances. **The constancy is downstream of both.**

## What it actually is — run 3, 132 readings

```
pre_state=e1996f82…  bonded=2  recorded=1  justifications=2  fringe=e1996f82  rejected=101
pre_state=e1996f82…  bonded=2  recorded=1  justifications=2  fringe=e1996f82  rejected=104
…
pre_state=e1996f82…  bonded=2  recorded=1  justifications=2  fringe=e1996f82  rejected=398
```

Two facts, and together they are the mechanism:

* **`pre_state` equals `fringe`** once the base settles — so the merge is returning **its base unchanged**,
  and the conflict scope is contributing nothing.
* **`rejected` grows monotonically, 6 → 398** across ~130 proposals. The merge is refusing the conflict
  scope's deploys wholesale, and refusing more of them as the chain goes on.

The fringe itself started at `0e5751c0` — **the empty state**, `empty_state_hash_fixed()` — which is where
finality had stopped on this arm.

## Why, and what it is not

The A2 arm **injects an equivocation from validator 1 on every block**. The H-1 gate refuses each twin, so
that validator's messages stop advancing; finality therefore freezes — the mechanism #148 measured. With
finality frozen, the last finalised fringe is fixed, the merge's base is that fringe's state, and the
proposer's conflict scope is refused. Its pre-state is the base for ever, so a `Slash` carried in a block
that is not in the base is **invisible to the next proposer**, which re-proposes it. Idempotently: the
offender is already out of the pool, so `slash` confiscates nothing, both nodes replay it identically, and
the chain advances throughout — which is what the arm reported from the start.

**So C201 is not a defect in the slashing path.** Every layer that can be tested in isolation is correct,
and each now has a test. It is a **coupling**: a chain that has stopped finalising also stops applying its
own recent blocks.

## What is registered separately, because it is a different claim

**The merge refusing an entire conflict scope is nobody's stated property**, and it is what makes a
non-finalising chain stop applying its own recent blocks. That is a claim about `MergeScope`'s resolution
rather than about slashing, so it gets its own row (C204) rather than being folded into this one — the same
shape #148 closing into #149 took. What is *not* claimed there: that the refusal is wrong. A merge that
rejects every concurrent writer and answers with the agreed base may be exactly right; what the row owes is
the statement, and a fixture that says when it happens.

## One caution for whoever runs this next, from this run's own mistakes

The first version of the instrument labelled the *number of justifications* `block=`, which reads as a
block number and is not. It was caught by pairing the fold's lines with the proposer's own `block #N` lines
rather than by reading the instrument alone — the same lesson the A1/A2 arms already carry, that a driver
reporting an absence must read evidence its own command cannot fabricate.
