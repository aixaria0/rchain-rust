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

**Again on `9a75d45`** (C210 added: a not-due propose is retried by the node itself), to check the retry
changes nothing on these arms: R1/R2/R3 finalised 24/23/23 with no escapes
(`n213-blocks/9a75d45-20261004T002358Z-rotation/`), and Arm 2 passed (a) 1 → 2, (b) 2 → 5, (d)
(`n213-blocks/9a75d45-20261004T003742Z/`). A validator bonding onto the running net is
[`n220-join-results.md`](n220-join-results.md): all four conditions pass.

**Still not run:** delivery delay, a long absence (Arm 2's restart came 18 s after the kill), and a bonded
validator that never speaks.

---

# Re-verification, 2026-10-04, on merged `dev` (`07af032ad`)

The findings above are the fixer's, read from their artefacts. This pass re-ran the two arms that decide
what can be claimed. Tree `07af032ad`, image `363c0588…`, and the report below is generated from the
transcripts, not from a commit message.

## Arm 1 — the counter-example this page published now passes

Six deploys to **one** validator, all three trigger patterns, zero escapes:

| arm | deploys to | before (`3708eab7b`) | **after (`07af032ad`)** |
|---|---|---|---|
| R1 | the bootstrap | none | **finalised 24** |
| R2 | rotating | 23 | **finalised 23** |
| **R3** | **validator-1** | **none, twice** | **finalised 23** |

Artefacts: `n213-blocks/07af032ad-20261004T074618Z/`. **R3 was the published counter-example and the fix
closes it.** The mechanism the fixer gives for the first cut's failure — reading the round's snapshot,
which before the first round closes is the genesis alone — is consistent with everything measured here.

**A residual, found per-deploy and not visible in the arm's last number.** Reading each deploy's own block
against the finality reached:

| arm | user-deploy heights | last finality | verdict |
|---|---|---|---|
| R1 | 1, 5, 9, 14, 18, 24 | 24 | all six finalised |
| R2 | 1, 5, 8, 14, 18, 24 | 23 | **the sixth is included and not finalised** |
| R3 | 1, 4, 7, 14, 17, 24 | 23 | **the sixth is included and not finalised** |

On a quiet `--no-autopropose` net production stops within a few heights of the last deploy and finality
lags ~4, so a deploy landing in the final few heights is included and never finalised. The issue's
criterion-1 witness is about *one* deploy and holds; "every deploy finalises" is stronger than the row
claims and held in one of three arms.

## Arm 2 — the three cases, with the witness this time

**The instrument was wrong twice before it was right, and both defects are recorded because the earlier
"PASS" depended on them.**

1. **v1 keyed case (a) to the wrong observation.** It waited for finality to pass the *pre-kill baseline*,
   not for the deploy's block. In the run it was used on, the post-kill deploy sat at height ≥ 6 and
   finality never passed 5 — so it printed `PASS` with nothing of the deploy finalised. Re-keyed to the
   deploy's own block.
2. **v2's block finder was broken.** `GET /api/blocks/{depth}` was asked for a fixed 50, and — the same
   trap `n149-sample.py` documents — a refusal was read as "no block". Then the floor was taken from
   `latestBlockNumber`, which is `max_height + 1`, so a deploy at genesis was searched for with `n > 1`
   and its own block at height 1 was excluded. Both are fixed, and a refused read is now reported as an
   **instrument error** rather than as a verdict on the chain.

Two void runs are kept under names that say why: `07af032ad-VOID-instrument-depth/` and
`07af032ad-VOID-floor-offbyone/`.

**The corrected run** (`07af032ad-20261004T083816Z/run/witness.txt`): every deploy's own block was found,
and every one finalised.

```
[baseline]      block  1 (fb5060a5ac1c…) carries it — included: yes → PASS: finalised (finality 1)
[absent-deploy] block 14 (7d7324d9d260…) carries it — included: yes → PASS: finalised (finality 14)
[post-restart]  block 18 (9d14cc0333d8…) carries it — included: yes → PASS: finalised (finality 19)
CASE (d) PASS: within the 900s budget
```

The absent-deploy is sent with `validator-2` stopped and lands at height 14 above a tip of 13: **a deploy
accepted while a validator is absent, included *and* finalised** — which is conditions (a) and (c), and it
is the observation the earlier recorded PASS did not have.

**One discrepancy, recorded and not smoothed.** `GET /api/is-finalized/{hash}` agreed for the
post-restart block (`true`) and disagreed for the other two (`false`), where `last-finalized-block` had
reached or equalled their height. That is consistent with a height-level witness being weaker than a
block-level one — several blocks share a height, and the fringe may confirm a *layer* one step after the
number moves. The issue names the height-level witness, so the conditions are met as written; the block-level
check is the stronger one and is not consistently satisfied. It is a question, not a refutation.

## Limits of this re-verification

One tree, one host, one attempt per arm; N=3 only. The restart came 18 s after the kill, so "a long
absence" is still unrun. The silent joiner is still unrun. Nothing here measures safety (TE-2).

---

# Re-run, 2026-10-04 (tree `a76648ca1`) — the kill/restart cases pass on the current tree

Artefacts `n213-blocks/a76648ca1-20261004T153629Z/`, image `f18c0071…`, node binary recorded in the
manifest. **The rig as it stands measures cases (a)/(c)/(d) — the kill/restart shapes — and not the
six-deploy arms the sections above describe**; that shape moved to `n214-rotation-run.sh`. Recorded
because a results file that describes a rig it no longer matches is the drift this audit keeps finding.

| case | reading |
|---|---|
| (a) the survivors finalise past the kill | the deploy accepted **while `v2` was stopped** is included in block 6 and **finalised** (finality 11) |
| (b) the restart needs no operator action beyond `start` | the post-restart deploy is included in block 25 and **finalised** (finality 26, `is-finalized=true`) |
| (d) the whole sequence inside the budget | PASS, well inside the 900 s budget |

That is A2.1 and A2.3 re-confirmed on a tree carrying every fix since — the same two readings the page
records from `07af032ad`, one tree later.

## One instrument inconsistency, recorded rather than smoothed over

The **baseline** deploy's line prints `PASS: block 1 finalised (finality 1; is-finalized=false)` — a
`PASS` beside a second witness that disagrees with it. The verdict rests on
`await_finality_at_least(block_number)` (finality ≥ the deploy's block number), and `is-finalized` is
printed as extra colour; on the two cases that matter — blocks 6 and 25 — the two agree
(`is-finalized=true`), so nothing in the table above rests on the disagreement. But a line that prints
two witnesses where one is false and calls it a pass is the shape this audit has been bitten by before,
and it is named here rather than left for the next reader to notice.
