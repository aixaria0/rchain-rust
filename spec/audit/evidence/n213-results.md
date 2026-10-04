# n213 — results: the C209 fix (PR #219) partially passes its own live falsifier

Protocol: [`n213-preregistration.md`](n213-preregistration.md).
Artefacts: `n213-blocks/3708eab7b-20261003T180638Z/` (run 1) and `…-20261003T182139Z/` (run 2), two
independent runs of the same rig on the same tree.

**Tree `3708eab7b`** — `ed65317c5` + the C209 fix, one commit, no other Rust change. Image
`sha256:66966d41…`, node binary `sha256:cbea4a79…` (the unfixed tree's binary is `38a1a32c…`; both
recorded in the sampler headers). Rig: 3 validators at 100/100/50, `--fresh`, `--propose-on-deploy`,
`--autopropose` omitted, `--attest-on-new-blocks`, `--epoch-length 10`. Six deploys twelve seconds apart.

**Result in one line: the fix rescues two of the three trigger patterns and does not rescue the third.**
#219's own stated falsifier — *"send every deploy to one node. Each deploy must finalise"* — **fails**.

## Arm 1 — the trigger-pattern experiment, before and after

| arm | deploys to | **before** (`ed65317c5`) | **after** run 1 | **after** run 2 |
|---|---|---|---|---|
| **R1** | the bootstrap (validator 0) | 0 finality | **475 samples, last finalised 24** | **472, last finalised 24** |
| **R2** | rotating 0 → 1 → 2 | 391, last 3 | **475, last finalised 23** | **475, last finalised 23** |
| **R3** | validator-1 | 0 finality | **0, none** | **0, none** |

Reproduced twice, independently, on the same tree. Before/after differ by exactly the C209 commit.

## What R3 actually does

The two runs agree block-for-block on the shape, so the failure is a mechanism and not a fluctuation:

| arm | blocks by sender (genesis sender = `04f700a4`, the bootstrap) |
|---|---|
| **R1** | `04f700a4` **28** · `04dbe32c` 27 · `04d8b6c3` 27 — all three speak at every height |
| **R3** | `04f700a4` **1** (genesis only) · `04dbe32c` 5 · `04d8b6c3` 1 |

**The bootstrap never produces a block after genesis.** In R3 the height-1 layer has two blocks, not
three; every later block justifies that same two-sender layer (`parents=2` at heights 2–5, where R1 has
`parents=3` throughout); validator-1 escapes six times moving the tip on a parent set that cannot close;
and the height stalls at 6 with nothing finalised.

The gate's own line says the same thing: `0 of 250 (0 full partition(s) among 1 candidate(s))` — one
candidate, no full partition.

## Why this matters

**It is a counter-example to the falsifier #219 names for itself**, and it is the shape the issue is
about: a chain in which one validator is silent produces nothing and finalises nothing, even though the
other two hold 150 of 250 and the survivors' own stake is not the obstruction. It is *not* the same
failure as before — before, all three spoke and the licence lapsed; here one validator never speaks at
all, so the round cannot close for a different reason.

**It is also the class #219's own pre-registration says it cannot see**: its in-process network
`quiet_chain_tests` states its scope as *"What it does not model: delivery delay and reordering, and the
store reads `create_block` makes."* A validator that never speaks is exactly a delivery-shaped failure,
and in-process every block reaches every validator at once.

**The mechanism is not yet located.** The bootstrap's tap should fire on validator-1's height-1
block (`attest_warranted` is per-sender and its map for that sender is empty), and with C209's licence
the guard should not suppress it. Something else stops it, and the run cannot say what — this is the
ambiguity the pre-registration predicted, and the resolution is the diagnostic line the issue asks for
(#213: name `nothing_to_finalize`, `new_state_transition`, `quorum_reachable`, `cadence_due`, the round's
sender set, and the escape counter at the suppression site), then this arm again.

## Arm 2 — #213's kill/restart conditions: **not run**

The pre-registration's second arm (kill one of three, deploy after the kill, restart it) was **not
executed**. Arm 1 already refutes #219's own falsifier, so running more arms against a fix that has
failed its stated test would have spent the rig's time on a settled question. It is not reported as
passed or failed — it is **not run**, and that is stated here rather than left to be inferred from its
absence.

## Consequence for the plan

The approved plan says: *if a falsifier fails, #219 is not merged and the counter-example is the
finding.* **#219 is not merged**, and the merge decision now rests on whether this failure is judged
in-scope for C209. Two readings, and they are the maintainer's:

1. **#219 is a strict improvement** — it fixes the bootstrap and rotation cases the issue's live report
   named ("deploys sent to one node never finalise") and it changes no validity rule. Merge it, and open
   the R3 shape as its own finding with this transcript.
2. **#219's falsifier is the whole claim** — *"send every deploy to one node"* is not satisfied while R3
   fails, so the row is `in progress`, not `done`.

The evidence supports either; what it does not support is calling the falsifier met.

## Limits

One host, three validators, mesh complete, no delivery delay injected. Two runs per arm. No bond was
attempted and no fresh-store join. Nothing here measures safety (TE-2). The R3 failure is reproducible
but its mechanism is unlocated, and that is stated as a gap rather than filled with a guess.

## Rerun on `881066f` (C209 complete), 2026-10-03/04 — all arms pass

The R3 mechanism above was located with the diagnostic line `attestation at tip N: withheld|licensed — …`
and fixed in #219's second commit: the genesis signer read its own genesis as "just spoke" and nobody else
as moving, because the tip, the live set and the cadence still came from the round's parents. Every guard
input now comes from `latest_msgs`. Same rig, same flags, binary built natively from `881066f` (Docker Hub
refused this container), one run per arm.

| arm | deploys to | **`881066f`** | artefacts |
|---|---|---|---|
| R1 | the bootstrap | h 28, finalised 24, quiet for the 90 s read window | `n213-blocks/881066f-20261003T220046Z-rotation/R1/` |
| R2 | rotating | h 27, finalised 23, quiet | `…/R2/` |
| R3 | validator-1 | **h 27, finalised 23, quiet** (was `none`) | `…/R3/` |
| Arm 2 (a) | kill validator-2, deploy | finality 1 → 2 past the kill | `n213-blocks/881066f-20261004T000955Z/` |
| Arm 2 (b) | restart it, deploy | finality 2 → 5, all three nodes at h 9 / finalised 5 | same |
| Arm 2 (d) | bounded | within the 900 s budget, no escape lines | same |

In R3 the bootstrap's first decision is `licensed … quorum_reachable=true … round=[04f700a4]` — the round
is its genesis alone, the exact state in which the first cut read `quorum_reachable=false`.

**A correction to the pre-registration**, which is frozen and so is corrected here: `--attest-on-new-blocks`
is not required. The clap flag only ever *adds* `true` (`config_mapper.rs`'s `flag`), and the merged
default is `true` (`node/src/configuration/defaults.conf:16`). The rig passes it anyway, which is harmless.

**Still not run:** a bond onto the running net, a fresh-store join, delivery delay. Arm 2's restart came
18 s after the kill, so it does not measure a long absence.
