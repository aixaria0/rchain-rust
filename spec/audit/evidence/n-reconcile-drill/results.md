# The reconciliation drill: the plan is right, and the recovery cannot be demonstrated — with both reasons

**Result: the plan phase is verified; the acceptance is NOT demonstrated, and this file says why.** Two
independent reasons, one about the instrument and one about the design, and the second is the more
important finding.

**Rig.** Four-validator devnet, `--epoch-length 10 --no-autopropose --fresh`, with the C215 divergence
injection armed on the joiners (`--merge-divergence-injection 2/3/4`; the survivor deliberately clean —
see "the first staging" below). The tool is `tools/reconcile-network.sh` on this branch. The raw
material is [`why-no-resync.txt`](why-no-resync.txt).

## What was verified: the plan phase

Run against a genuinely diverged net — heights 3/4/4/3, the clean survivor rejecting the injected
joiners' blocks (14 `state-hash disagreement` lines on it), no node finalising anything:

```
== 1. what each node is showing ==            A h=3   V1 h=4   V2 h=4   V3 h=3   (finalised: none)
== 2. provable equivocation …                 none — every sender has at most one block per height
== 3. the meet (stake-weighted) …             height 0, block c01a6091…, vouched for by A V1 V2 V3
                                              stake: 400 of 400 = 100% — a strict supermajority
== 4. what is above the point …               12 deploy(s), per-block record written
== plan only ==                               exit 0, nothing touched
```

Every part of that is the intended behaviour: no equivocation is invented, the meet is the deepest
height a *strict supermajority of stake* agrees on, and when nothing is finalised that height is
**genesis** — which is the honest answer rather than a winner picked by hand. The tool then exits
without touching anything.

## What was not: the recovery

The joiners were wiped (the devnet's own `reset`, which is the same stop/empty/restart the tool's
`--apply` performs on a droplet) and left to resync from the clean survivor. **They did not converge,
and they did not even validate the survivor's chain:**

```
WARN [casper.blocks.BlockProcessor] Block #1 4ea92991… from 04ea3ce0… failed validation:
     the block's rejected-deploy set does not match its parent
WARN [casper.interpreter.validate] state-hash disagreement on rejected-deploys: block #1 4ea92991… by 04ea3ce0
```

And all four answer `"Finalized fringe is not available."`.

### Reason 1 — the instrument stages *invalidity*, not the ambiguity the incident had

**The injection reaches the block's content.** A proposer's `rejected_deploys` is *computed by its own
merge*, under its own perturbed cache, and then written into the block it publishes. So every block an
injected node produces carries a rejected-deploy set that a clean node cannot reproduce — permanently,
because the set is in the block. The survivor's DAG holds those blocks, so a wiped joiner revalidates
them and refuses each one.

That is **stronger than the incident**, and the difference matters. In the live divergence every node was
honest and each block was valid *to its proposer*; the disagreement was an ambiguity in a derived cache.
Here the chain is un-validatable by anyone who does not share the perturbation. A staging instrument
should not be able to make the chain worse than the thing it stages.

**The first staging made this worse and taught the lesson**: with all four nodes injected, the *survivor
itself* was tainted, and nothing could ever agree with it. Injecting only the joiners was the correction
— the survivor must be clean for a recovery to have a target — and it is not sufficient, for the reason
above.

### Reason 2 — the recovery presupposes a finalised fringe, and TE-1's state may have none

This is the finding, and it is about the design rather than the drill. All four nodes answer
`"Finalized fringe is not available."` — which is *what four divergent heads with frozen finality means*.
A joiner's sync path is keyed on the finalised fringe: it asks the peer for a fringe and restores from
it. With no fringe, the wipe leaves a node with an empty store, a genesis, and peers it cannot sync from.

The TE-1 witness already recorded this dependency in its own words — *"A restoring or joining node syncs
from the approved-genesis / finalised fringe. While finality is stuck that path is closed"* — and this
drill is the measurement of what that costs: **#287's recovery — wipe, resync from the finalised fringe,
verify — has nothing to resync *from* in exactly the state it exists for.** The meet at genesis is
computable, but there is no "sync from genesis" or "sync from an agreed block" path for the joiners to
take.

So the acceptance (*"a network in that state converges to one head, with agreeing block hashes, without a
genesis"*) is not reachable by the mechanism as designed. Closing it needs one of:

1. **a fringe-free restore path** — a joiner that can sync from an arbitrary agreed height (the genesis
   block, or the meet) rather than from a finalised fringe; or
2. **a store-level restore** — the operator copies the survivor's data directory onto the joiners, which
   is what the manual procedure actually did on the old chain (it is not a sync at all); or
3. **a stated limitation** — the tool refuses earlier and says that a net with no finalised fringe cannot
   be reconciled this way, which is at least honest and is close to what this branch's tool already does
   with its meet.

This is a design question for #287 and it is now a measured one rather than an assumed one.

## What this does not show

- **The tool's `--apply` path was not exercised end to end.** The devnet's own `reset` was used for the
  wipe, because the tool's docker control path performs the same three steps; the systemd path is
  untested here and belongs to a droplet.
- **The plan phase's correctness on a *healthy* net** — a meet above genesis, a lagging node that must
  not drag the network back, the pool-disagreement refusal — is unexercised. This staging has no
  finality at all, so every run lands at genesis. Those cases need a net that finalises and then a
  deliberate lag or partition.
- **The equivocation check has never fired.** No injection here produces two signed blocks by one sender
  at one height; the existing `--equivocation-injection` would, and that arm is unrun.
- One host, one attempt per staging.
