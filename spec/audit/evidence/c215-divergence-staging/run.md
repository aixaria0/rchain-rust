# C215's mechanism, staged on a running four-validator net — injected, and self-disclosing

**Read the disclosure first.** The divergence below was **injected**, by a flag built for it
(`--merge-divergence-injection <n>`, devnet-only, refuses to arm without `--dev-mode`). It is not a
natural occurrence and this file does not claim one. What it establishes is narrower and still worth
having: the mechanism C215 is about, reproduced in process by
`casper/tests/merge_determinism.rs`, **also reproduces between four live nodes that are running the real
merge**, and the node's own diagnostics name it.

**The capture is [`capture.txt`](capture.txt)** — and what it holds is worth stating exactly, because it
is not the raw logs: it is the readings collected from the four nodes (their version, their heights and
stall strings, their injection lines) and **the first two disagreement lines per node, truncated at 220
characters**. Two of those lines are quoted in full in this file. The containers were removed at the end
of the run, so the untruncated logs are not recoverable — a re-run would produce *different* disagreement
lines rather than the same ones, which is why this file quotes what it saw rather than re-collecting it.

## Why this was staged

The row's close condition is a reproduction, and it has two halves that can be met in two different
places. The in-process half is the two tests on this branch: one block set, two arrival orders, two
different `fringe_states` at one key, and therefore two different merges. The other half is that this
survives contact with a running network — four nodes, real gossip, real merges, real validation — because
a test that drives `insert` directly cannot show what the *nodes* do with the disagreement.

Before this, the only live artefact of the class was TE-1 itself: **four divergent heads and a chain that
did not recover for thirteen hours**, with no way to reproduce it on demand.

## The configuration

| | |
|---|---|
| image commit | `da032f9e7307e3bb26d9382e4399a1d9de37fee0` (this branch's head, read from each node's own `/api/v1/status`) |
| topology | `tools/devnet.sh up --validators 4 --epoch-length 10 --no-autopropose --fresh` |
| injections | `DEVNET_EXTRA_FLAGS_devnet_bootstrap="--merge-divergence-injection 1"`, and `…_validator_1/2/3` = `2`, `3`, `4` |
| genesis | fresh, the standard ceremony |

All four armed, and each said so in its own log — the instrument's disclosure line, one per node:

```
[merge-divergence-injection 1] perturbed the fringe record for block c01a6091fbde1a10… at key …
[merge-divergence-injection 2] perturbed the fringe record for block c01a6091fbde1a10… at key …
[merge-divergence-injection 3] perturbed the fringe record for block c01a6091fbde1a10… at key …
[merge-divergence-injection 4] perturbed the fringe record for block c01a6091fbde1a10… at key …
```

## What the run did

Nine deploys across the four nodes, then the readings below. **Every node rejected other nodes' blocks,
and the two faces TE-1's log shows both appeared:**

```
WARN [casper.interpreter.validate] state-hash disagreement on rejected-deploys: block #1 eeb29194… by 04d8b6c3
  — this node's merge computed 3 rejected (d102, d104 only here), the block claims 2 (d103 only there);
  fringe_state=0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8
  prev_fringe_lookup=0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8

WARN [casper.interpreter.validate] state-hash disagreement on pre-state: block #2 41459ed0… by 04d8b6c3
  — declared 477218ed… vs recomputed 27d90723… (pre_state=27d90723…
  fringe_state=0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8 prev_fringe=[])
```

| node | height | disagreement lines |
|---|---|---|
| devnet-bootstrap | 4 | 9 |
| devnet-validator-1 | 3 | 5 |
| devnet-validator-2 | 3 | 5 |
| devnet-validator-3 | 3 | 5 |

**`/api/last-finalized-block` answered `"Finalized fringe is not available."` on all four** — the same
answer the TE-1 witness records as the dependency that closes the recovery path ("restarting recovers a
*node*; nothing recovers the *chain*"). The stall diagnostic on each node:

```
a layer exists but its supporting stake is not a supermajority — 0 of 400 (0 full partition(s) among 0 candidate(s))
```

## Why this says something the in-process tests cannot

**The diagnostics name the mechanism, and they name the same map on both faces.** Every line carries
`fringe_state=` and `prev_fringe_lookup=` — and the two are the *same hash* on every disagreement above,
which is the point: the node and its peer are looking at the *same fringe key* and reading different
values out of it. `rejected-deploys` is what `rejections_for` reads (the `only here` / `only there`
enumeration is the two nodes' caches disagreeing); `pre-state` is what `get_pre_state_for_parents` reads
from the record's `state_hash`. One map, both symptoms.

That is exactly what `fringe_states` being keyed by the fringe **set** while carrying per-block values
predicts, and it is what the two in-process tests assert one link at a time.

## What this does not show

- **The divergence is injected.** The injection is the *cause* the tests identified, not a symptom
  manufactured to look like one: it makes four nodes' fringe records differ at the keys they share, which
  is what a different arrival order does naturally. Nothing fabricates a state hash or a block — the
  disagreements above are computed by the real merge from real records. But a net that diverges *because
  it was asked to* is not a net that diverged on its own.
- **Whether the natural collision is reachable on a live chain is not settled here.** It needs the
  pre-restart capture the TE-1 witness names (two blocks at one height, by different senders, sharing a
  fringe set with different `rejectedDeploys`), which is a grep and not a run.
- **The reconciliation tool has not run against this state.** That is the drill, and this staging is its
  precondition.
- **One host, one run.** Four containers on one machine; the timeouts and the heights are this box's.
- **The injection reaches the *blocks*.** Found later, by running the recovery drill against a net staged
  this way: a proposer's `rejected_deploys` is computed by its own merge under its own perturbed cache and
  then written into the block, so every block an injected node publishes is un-validatable by a clean
  node — permanently, because the set is *in* the block. The paragraph above says nothing fabricates a
  state hash or a block, and that is true of the *injection*; what it did not anticipate is that a
  perturbed cache is not private to the node holding it, because the node's next block publishes the
  difference. So this instrument stages a chain that is *worse* than a genuinely diverged one — see
  `spec/audit/evidence/n-reconcile-drill/results.md`, which is where the cost and the correction are
  recorded.
