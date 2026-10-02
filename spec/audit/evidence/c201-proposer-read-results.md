# The proposer's pre-state does not advance (C201, #150)

The measurement C201's row asked for once its fixture passed and **exonerated the merge**: the question
became *which hash the proposer reads*, and that is a live-run one, so this is a live run.

`spec/audit/evidence/c201/` holds the raw output: `proposer-read-run1.txt` and `-paired.txt` (the fold's
lines, and the same lines paired with the proposer's own "block #N"), and `proposer-read-run2.txt` (the
second run, which also logged the parent set).

## The instrument

In the fold itself, beside the `bonded` filter: the pre-state it asked, how many validators that answer
held, how many equivocations this node has recorded and which of them the filter **admitted**, and — in
the second run — the parent set the pre-state came from. Logged only when something has been recorded, so
a chain that has never seen an equivocation pays nothing.

## What it found

**The fold admits the offender on every proposal, from a pre-state that never changes.**

* Run 1: **140 readings**, the chain advancing from height 3 to **142**, and **one** distinct pre-state
  hash. Every line byte-identical.
* Run 2: **133 readings**, **133 distinct parent sets**, and **one** distinct pre-state hash.

**And the parent sets are all the same shape:** `[<advancing hash>@N, d8f23b38@7]` — one parent advances to
height 122 across the run, and the other is **pinned at height 7** for the whole of it. `d8f23b38@7` is
the only thing every one of the 133 sets has in common.

So the row's question is answered at one level and sharpened at another. It is **not** "which hash" in the
sense of a stale value: the proposer reads a pre-state that **does not advance while its parents do**. And
it is not the merge's *native fold*, which C201's fixture exonerates. What it is now is: **merging a set
that contains a stuck parent ignores the parent that moves.**

## What this does NOT establish

**Which side of `get_pre_state_for_parents` is at fault**, and the reason is specific rather than
open-ended. The site is its multi-parent branch (`casper/src/multi_parent_casper.rs`: the
`MergeScope::from_dag` + `MergeScope::merge` arm — the single-parent arm is a plain post-state read and
cannot be constant). Two readings survive:

* **the merge discards the advancing branch** — the scope `from_dag` derives from a parent set containing
  a far-behind block does not reach the blocks above the moving one, in which case the defect is in the
  scope derivation and C201 closes into a fix there;
* **the constancy comes from the caller or the rig** — the live arm's other validator self-harms, and a
  frozen fringe (which #148's mechanism produces) would anchor every merge at the same base.

**The check that distinguishes them is a unit fixture, not a rig, and it is small**: merge `{X@122, Y@7}`
and `{X@78, Y@7}` over one fringe and compare the state hashes. **Equal** ⇒ the merge is discarding the
advancing branch. **Different** ⇒ the rig's frozen finality is the next thing to measure.

## One caution for whoever runs it, from this run's own mistakes

The first version of this instrument labelled the number of justifications `block=`, which reads as the
block number and is not: the "constant `block=2`" in that log is *two parents*, not height 2. It was
caught by pairing the fold's lines with the proposer's own `block #N` lines rather than by reading the
instrument alone — which is the same lesson the A1/A2 live arms already carry, that a driver reporting an
absence must read evidence its own command cannot fabricate.
