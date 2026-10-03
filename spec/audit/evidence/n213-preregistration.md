# n213 — pre-registration: verify the C209 fix on a live net

**Frozen before the run.** Issue: [#213](https://github.com/rchain-community/rchain-rust/issues/213).
Fix under test: [#219](https://github.com/rchain-community/rchain-rust/pull/219), register row **C209**.

## Why this run exists

#219 changes `attestation_suppressed`'s licence so it is read from `latest_msgs` — everything the node has
seen — instead of the round's parents, bounded by `ATTESTATION_HORIZON = 3 × LIVENESS_WINDOW`. It is
node-local, block validity is unchanged, and it needs no new genesis. CI is green, and it carries an
in-process network (`quiet_chain_tests`) with four tests.

**Its own stated gap is what this run closes:** *"Not yet run on a live net. The live falsifier: on three
validators at 100/100/50 with `--no-autopropose`, send every deploy to one node. Each deploy must finalise,
and the chain must go quiet afterwards."*

## The trees

| | tree | what it is |
|---|---|---|
| **before** | `ed65317c5` | the merge-base; measured already by `n214-results.md` and `n214-rotation-results.md` |
| **after** | `3708eab7b` | `ed65317c5` + C209 — one commit, no other Rust change |

Both are recorded in every artefact, with `git rev-parse HEAD` **and** `origin/dev` (an absence claim made
from a stale clone is a fact about the clone; see the acceptance page §0.9).

The **before** readings are already committed and are not re-run: `n214-rotation-results.md` arm **R1**
(six deploys to the bootstrap) → **0 numeric finality samples**, gate at `0 of 250 (0 full partition(s)
among 3 candidate(s))`; arm R2 (rotation) → finalised 3.

## Rig

```
tools/devnet.sh build                                  # from the tree under test
DEVNET_EXTRA_FLAGS=--attest-on-new-blocks \
tools/devnet.sh up --validators 3 --fresh --stakes 100,100,50 --epoch-length 10 \
    --no-autopropose --propose-on-deploy
```

`--attest-on-new-blocks` is **required** and is not a `devnet.sh` flag: the tap is gated on
`attest_on_new_blocks && !no_attest_on_new_blocks` and the positive clap flag defaults false, so without it
no node ever attests and nothing finalises at any N (recorded in `n214-preregistration.md`).

`--no-autopropose` (i.e. omitting `--autopropose`) is required: with it on and a deployer key, the dev-mode
dummy deploy is injected into every empty-pool block, `new_state_transition` is pinned true, and the guard
under test is out of reach.

## Arm 1 — #219's falsifier (the trigger-pattern experiment, re-run)

`n214-rotation-run.sh`, unchanged, against the **after** tree. Three arms, six deploys twelve seconds apart:

| arm | pattern | before | expectation on the fixed tree |
|---|---|---|---|
| **R1** | all six → bootstrap | **none** | every deploy finalises, then the chain is **quiet** |
| **R2** | rotating bootstrap → v1 → v2 | finalised 3 | every deploy finalises |
| **R3** | all six → validator-1 | **none** | every deploy finalises |

**Pass.** On R1 and R3, `last-finalized-block` reaches **or passes the height of the block carrying each
deploy** — the deploy finalising is the witness, never "the height stopped moving". **"Quiet" is
distinguished from "stalled"**: after the last deploy finalises, the deploy pool is empty and the height is
flat. A chain holding a deploy it cannot include is a **fail**, and this is the distinction the whole issue
turns on.

**Void.** Fewer than 3 nodes past genesis within 180 s; a deploy refused; or the sampler header's tree not
the recorded one.

## Arm 2 — #213's three close conditions, bounded

`n213-run.sh`, one rig, three phases, a wall-clock cap on the whole run:

| case | action | witness |
|---|---|---|
| **(a) kill** | reach 3 live and a settled baseline; `devnet.sh stop 2` (the 50-stake validator); **then deploy** | a block carrying that post-kill deploy **finalises** (`last-finalized-block` ≥ its height) |
| **(b) return** | `devnet.sh start 2` | production and finality resume with **no reset, no re-genesis, no operator action** — a new height is finalised after the restart |
| **(c) absent-deploy** | a deploy submitted **while** the validator is away | it is included and finalised once the survivors can finalise |
| **(d) bound** | the run has a hard wall-clock cap | a timeout is a **failure**, not a pass |

Case (c) is the one that separates "the pool cannot be drained" from "the pool is drained and the result
cannot be finalised" — #213's own words.

**Void.** As arm 1, and additionally: if the pre-kill baseline never reaches a finalised height, the kill
has nothing to advance from and the arm is void.

## What this run does not measure, and says so

- N=5 and N=8. The rig is three validators.
- Delivery delay and reordering: every node is on one host and the mesh is complete.
- Safety (TE-2): nothing here measures whether two validators can each finalise a conflicting block.
- The **fresh-store join** leg (`devnet.sh reset`) is not exercised; case (b) is a **restart** with the same
  store, which is what #213's close condition 2 names (*"the killed validator restarts and rejoins"*).

## Instruments

- `spec/audit/evidence/n149-sample.py` — the shared sampler: `height`, `finalized`, `alive` per node each
  second, plus the block-hash union `blocks.tsv` (never `latestBlockNumber`, which counts heights).
- `n214-rotation-run.sh` — arm 1, unchanged.
- `n213-run.sh` — arm 2.
- The gate's own `finality did not advance at tip N: …` line is captured by both, since it is the only
  signal that separates "no full partition" from "a supermajority refused".

## Outcome vocabulary

`pass` / `fail` / `void`, per arm, with every reading citing the tree it was taken on. #213 closes only if
arm 1 passes on R1 **and** R3, and cases (a), (b) and (c) each produce a positive witness.
