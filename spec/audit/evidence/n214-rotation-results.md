# n214-rotation — the trigger-pattern experiment

Rig: `n214-rotation-run.sh`. Artefacts: `n214-rotation-blocks/ed65317c5-20261003T165706Z/`.
Tree `ed65317c5` (HEAD, carrying #216 = the C208 fix). Three validators at 100/100/50, `--fresh`,
`--propose-on-deploy`, `--autopropose` omitted, `--attest-on-new-blocks`, `--epoch-length 10`.
Six deploys, twelve seconds apart, in three patterns.

**Purpose.** To test the mechanism the code trace names — that `attestation_suppressed` is
`!(new_state_transition || cadence_due)` under `paced && quorum_reachable`, so with no deploy-bearing
parent and no validator behind a tip that suppression itself freezes, **every validator suppresses and
the round never closes**. The one manipulation ever observed to turn the stall off (Jim, #214) was
triggering the validators in rotation; this converts that observation into a measurement.

## The reading

| arm | trigger pattern | finality | last finalised | escapes |
|---|---|---|---|---|
| **R1** | all six deploys → bootstrap | **none** (0 numeric samples) | — | 2 |
| **R2** | rotating: bootstrap → v1 → v2 → bootstrap → v1 → v2 | **395 numeric samples** | **3** | 0 |
| **R3** | all six deploys → validator-1 | **none** (0 numeric samples) | — | 6 |

**Rotation turns finality on; a single trigger point does not, whichever validator it is.** R3 rules out
"the bootstrap is special" (it targets validator-1 and still stalls); R1 rules out "validation-1 is
special". What matters is that *every* validator is asked, not which one.

## The discriminator, and what it says

The gate's own log, per arm:

```
R1  tip 0: 0 of 250 (0 full partition(s) among 0 candidate(s))
    tip 2: 0 of 250 (0 full partition(s) among 3 candidate(s))
    tip 3: 100 of 250 (1 full partition(s) among 3 candidate(s))     ← escapes: 2
R2  tip 0: 0 of 250 (0 full partition(s) among 0 candidate(s))
    tip 2: 0 of 250 (0 full partition(s) among 3 candidate(s))       ← then finality advances
R3  tip 0: 0 of 250 (0 full partition(s) among 0 candidate(s))
    tip 3: 0 of 250 (0 full partition(s) among 1 candidate(s))       ← escapes: 6
```

Three things follow.

1. **The fringe condition is not the blocker; the support map is.** Every line is the `Support` branch
   with `full_partitions = 0` — candidates exist (3 of them at tip 2) and **none is seen by the whole
   partition**. That is the composition the trace predicts: with only one validator speaking at the tip,
   `calculate_next_fringe_support_map`'s `parents_of_parent` walk yields `seen_by = {genesis: {A}}`, one
   seer, so nothing is a full partition.
2. **The escape cannot help, and R1 measures exactly why.** In R1 the gate climbs to `100 of 250 (1 full
   partition among 3)` — *one* validator's stake, 100, short of the 167 needed. That is precisely the
   code trace's prediction for the escape's asymmetric parent set (`{A@3, B@2, C@2}`): the escaper alone
   is credited. The escape moves the tip and changes the gate's number from 0 to 100; it cannot reach 167.
3. **Suppression, not the round gate, is what stops the chain.** R2 — the arm that advances — logs
   **zero** escapes, because its validators are not blocked; R1 and R3 log 2 and 6, because a single
   trigger point *does* round-block its one speaker. The escape firing is a symptom of the wrong
   configuration, not the cure.

## What this establishes

The stall is **not** in the gate's arithmetic, the quorum, the live partition, C208, or the escape. It is
in the **trigger**: with `--propose-on-deploy` and no `--autopropose`, only the validator a deploy is
addressed to speaks, the round never closes, and the fringe — which needs the *next* round's snapshot —
never advances. Rotating the trigger closes the round and finality advances.

This is the `[self-referential]` shape the acceptance page's §2.4 already names, and it is why #215's fix
(the round gate's wall-clock escape, *"needs a clock, not just a supply of attempts"*) does not touch it:
**the same defect exists one layer up, in the attestation guard**, where the pace bound
(`cadence_due`) is measured against a tip the suppression itself freezes. #215 gave the round gate a
clock. The attestation guard still has none.

## Limits

One attempt per arm, one tree, one host, N=3. R2 finalises to height 3 and is read over a 90 s window —
enough to show the gate advancing, not enough to characterise its steady state. A1.2 (N=5) and A1.3 (N=8)
are untouched. Nothing here measures safety (TE-2).

---

# Re-run, 2026-10-04 (tree `513e2192b`, Rust identical to `dev`) — A1.5's residual survives, narrower

Artefacts `n214-rotation-blocks/513e2192b-20261004T154216Z/`, image `f18c0071…`. Six deploys twelve
seconds apart per arm, `--no-autopropose --propose-on-deploy`, `--epoch-length 10`.

**This is A1.5's instrument, and its row cited the wrong rig for it.** A1.5 cites
`n213-blocks/07af032ad-…`, which carries `R1|R2|R3` — the six-deploy shape the **old** `n213-run.sh` had.
The current `n213-run.sh` is the kill/restart rig (cases a/c/d), so the cite names a file that no longer
has a rig producing its shape, and `n214-rotation-run.sh` is where that shape lives now. The per-deploy
reading below is recomputed from `blocks.tsv` + `series.tsv` + `marks.tsv` with the rule the earlier
reading used: *a deploy's block is the first block first seen at or after the deploy instant with
`deploy_count ≥ 1`*.

| arm | last deploy's block | finality reached | verdict |
|---|---|---|---|
| **R1** (all to bootstrap) | 31 | **32** | all six finalised |
| **R2** (rotating) | 29 | **31** | all six finalised |
| **R3** (all to validator-1) | 19 | **17** | **the sixth is included and never finalised** |

**A1.5 still fails — in one arm of three, where the cited run had two (R2 and R3).** C214 moved it and did
not close it: the residual is the last deploy's block riding a chain whose finality has plateaued.

**R3's plateau is not a truncated read.** Its finality reads `17` with the chain at height `22` for the
last forty-odd samples, and the sampler's window runs past the arm's `end` mark by a margin — nothing was
still climbing when the window closed. The gate's own reason lines name partitions of `100 of 250` and
`50 of 250`: **the round did not close**, which is the shape the A1.5 row describes.

## Limits

One attempt per arm, one tree, one host, N=3 — the same limits the earlier sections carry. A single arm
differing between two runs is one sample, not a rate: what the two runs together say is that the residual
is real and its *frequency* moved from two arms to one.
