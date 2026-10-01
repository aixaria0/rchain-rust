# #139, Arm D' — reproduced: a node that LFS-syncs a mature chain cannot replay its own restored blocks

Protocol: `n139-fringe-divergence-preregistration.md` (frozen before the run).
Rig: `n139-mature-join-run.sh`. Artifacts: `n139-mature-join/048c2532f-20261001T101407Z/`.

**Result: 2 of 2 attempts reproduced, deterministically, and the line names the block, the deploy and
both hashes.** The bootstrap never disagreed with anything — the divergence is entirely on the joiner.

## What was run

```
up --validators 2 --stakes 1000,100 --epoch-length 10 --fresh      # mature the chain past two boundaries
tools/devnet.sh reset 1                                            # wipe validator-1's store, recreate it
                                                                   # with the bootstrap left running
```

`reset` is new here (#139's instrumentation PR) and its shape is the arm: `stop`/`start` cannot
substitute (a restarted container reuses its volume and would rebuild its stored chain, never syncing),
and `down`/`up` cannot either — it restarts the **bootstrap** too, so the joiner syncs a chain that is
still being replayed. Measured on the first version of this rig: the joiner logged
`LFS state is successfully restored.` and the arm said nothing, because what it restored was the
bootstrap's half-rebuilt chain.

## The observation, per attempt

| attempt | joiner synced to | joiner stuck at | bootstrap reached | disagreement lines on the joiner | on the bootstrap |
|---|---|---|---|---|---|
| 1 | 27 | **27** | 127 | 93 | 0 |
| 2 | 23 | **23** | 123 | 92 | 0 |

The joiner takes the LFS path (`LFS state is successfully restored.`, once per attempt), then stops
advancing — it produces no blocks and follows none — while the bootstrap runs on.

## The line, verbatim

```
2026-10-01T10:14:39.158Z ERROR [casper.blocks.BlockProcessor] Block 02907cc08d80e597e6349324f6592c863232d1eb6f4e7c44c2b7f09aa58d7420 processing error: validateBlockCheckpoint failed: regenerated mergeable channels for block 1a44d3d9bee01b17714ffff56b2567b7cbcf1c820cfc1fbbdd9dc3f16def9e20 but replay computed 2023ce01a50e39092aeb6db44f492675852b9561256bac5a3f55df325c0af294 instead of 0cda61ca8a6e573176da3b58f5846abffe37e476a5ab686803b6dcf68794b69e
```

That is the issue's close condition met on its own terms: **the block** (`02907cc0…` being validated,
`1a44d3d9…` the one whose replay diverged) and **the two hashes** (recomputed `2023ce01…`, declared
`0cda61ca…`). Attempt 2 carries the same shape with different hashes.

## The deploy is named by the run, not inferred

The joiner's log repeats one close-deploy line 93 times, against a chain whose boundaries are at 10, 20
and 30:

```
[pos] close_block 20 boundary=true epoch_length=10 max_active=100 bond=[1, 9223372036854775807]
```

`boundary=true` on the **epoch boundary** block — which is exactly the mechanism the preregistration
named: `close_block` anchors the next epoch's seed to the fringe state, so a node whose fringe state is
not the proposer's replays that block to a different post-state. (The `bond=[1, i64::MAX]` field is the
fixture's own `--stakes 1000,100` arithmetic, not a reading.)

## What it means

The three arms compose into one account, and none of them needs the other two to be argued:

- **Arm B** (in process, reproduced): `populate_dag` inserts every restored non-genesis block through
  `BlockMetadata::from_block`, which cannot know the fringe — so the node's `fringe_states` cache holds
  `fringe_hash_of(∅) → 0` and never the chain's real fringe key.
- **Arm A** (in process, reproduced): the post-state is a function of the fringe state, and
  `handle_errors` reports `Ok(None)` — `InvalidStateHash` — when two nodes derive different ones.
- **Arm D'** (this run): on a live network, the consequence is not merely a refused block but a node
  that **cannot index its own restored chain**, so it never produces and never catches up.

**This is a defect in the restore path, not the documented per-node-seed design.** The design departure
(the seed is derived locally, "nothing is published and nothing is verified") is what makes the
divergence *possible*; what makes it *happen here* is that a restored node has no correct fringe to
derive from, and there is a local fix for that. So the preregistration's decision rule #1 applies:
**fix `populate_dag`**, and §6 is not the disposition.

## What this run does not settle

- **It does not reproduce the #105 observation.** That was two *peers* disagreeing about a proposer's
  block; this is one node failing on its own restored chain. They share the mechanism and the status
  family, and #105's specific case is not what was reproduced here.
- **The fix's shape is not decided by this run.** Re-deriving a restored block's fringe means replaying
  the restored ancestry, which is the work LFS sync exists to avoid — so the fix is a real cost decision
  and is left to the next unit rather than chosen here.
- Two attempts of one configuration on one machine: this is the measurement the register asks for before
  a claim is written down, not a proof.
