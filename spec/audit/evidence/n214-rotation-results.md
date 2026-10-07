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

## The two accounts, reconciled — this is a second sample, not a dispute

jimscarver's regression run of the **same rig on the same configuration** (`n223-rejoin-blocks/regressions/
rotation/`, tree `f1ca009`, 11:36) records **all six finalising in all three arms**. Applying the identical
rule to his committed artefacts reproduces his figures exactly:

| run | R1 | R2 | R3 |
|---|---|---|---|
| `f1ca009` (jimscarver) | 45 ≤ **51** | 29 ≤ **31** | 25 ≤ **25** |
| `513e2192b` (this run) | 31 ≤ **32** | 29 ≤ **31** | **19 > 17** ✗ |

**Nothing in his run is in dispute and nothing in this one contradicts it**: the runs agree where they
overlap (R1 and R2 finalise all six in both), and they differ in **one arm of one run**. Three samples of
this rig now exist — `07af032ad` with 2 of 3 arms short, `f1ca009` with 0 of 3, `513e2192b` with 1 of 3 —
and what they say together is that **the quiet chain's tail is not reliably finalised**, which is why
A1.5 stays ❌ and is not retired by the green run, and why the red one does not establish a rate.

**R3's plateau is not a truncated read.** Its finality reads `17` with the chain at height `22` for the
last forty-odd samples, and the sampler's window runs past the arm's `end` mark by a margin — nothing was
still climbing when the window closed. The gate's own reason lines name partitions of `100 of 250` and
`50 of 250`: **the round did not close**, which is the shape the A1.5 row describes.

## Limits

One attempt per arm, one tree, one host, N=3 — the same limits the earlier sections carry. A single arm
differing between two runs is one sample, not a rate: what the two runs together say is that the residual
is real and its *frequency* moved from two arms to one.


# 2026-10-07 (tree `9f52d84d3`) — the tail is a fixed band, and the flicker is the slack

**What this adds.** The three earlier samples disagreed (2/3, 0/3, 1/3 arms short) and the page called
the residual **intermittent**. It is not. Re-reading all twelve arms of all four runs — including a
fresh three-arm run on `9f52d84d3` — the unfinalised tail is a **fixed band**, and whether a given
deploy is caught in it is decided entirely by how much production ran after it.

## The band

| run | arm | tip at rest | finality reached | tip − finality | sixth deploy's block | tip − block | verdict |
|---|---|---|---|---|---|---|---|
| `07af032ad` | R1 | 28 | 24 | **4** | 24 | 4 | OK |
| `07af032ad` | R2 | 27 | 23 | **4** | 24 | **3** | SHORT |
| `07af032ad` | R3 | 27 | 23 | **4** | 24 | **3** | SHORT |
| `513e2192b` | R1 | 36 | 32 | **4** | 31 | 5 | OK |
| `513e2192b` | R2 | 35 | 31 | **4** | 29 | 6 | OK |
| `513e2192b` | R3 | 22 | 17 | **5** | 19 | **3** | SHORT |
| `f1ca009` | R1 | 55 | 51 | **4** | 45 | 10 | OK |
| `f1ca009` | R2 | 35 | 31 | **4** | 29 | 6 | OK |
| `f1ca009` | R3 | 29 | 25 | **4** | 25 | 4 | OK |
| `9f52d84d3` | R1 | 55 | 51 | **4** | 45 | 10 | OK |
| `9f52d84d3` | R2 | 35 | 31 | **4** | 29 | 6 | OK |
| `9f52d84d3` | R3 | 29 | 25 | **4** | 25 | 4 | OK |

**The one rule that accounts for every arm**: a deploy finalises iff production ran **at least 4
heights past it**. `tip − finalized` is **4** in eleven arms and **5** in one; every run that was "all
green" left its sixth deploy 4, 6 or 10 heights below the tip, and every run with a short arm left it
**3**. The 2/3, 0/3, 1/3, 0/3 spread is the slack varying, not the node.

**And these are walls, not truncated reads.** Each arm sat frozen at its final reading for 37 to 110
one-second samples. The `513e2192b` R2/R3 arms' `stall-lines.txt` name partitions of `100 of 250` and
`50 of 250`, and their `escape-lines.txt` three `round gate escaped` lines — the signatures
`n214-rotation-results.md` already used to rule out a truncated read.

## Why the band exists, and why it is a lag rather than a loss

Read from the code, not inferred: the finalizer's fringe requires a candidate whose parents reach
**beyond** the next layer (`block-storage/src/dag/finalizer.rs`), so the messages of the last layer can
only be finalised by messages that do not yet exist; and on a quiet net nothing mints them, because the
round gate's escape (`casper/src/blocks/proposer/proposer.rs`) is only evaluated when something *asks*
the node to propose, and with `--no-autopropose` nothing does. So the last `LIVENESS_WINDOW`-ish heights
of an idle chain are unfinalisable **by construction**, and a deploy of which that is true is waiting,
not lost.

`n214-tail-lag-run.sh` measures which. In every arm it ran, the block sitting at the wall's tip — the
greatest height, unfinalised when the chain was quiet — was **finalised as soon as the chain produced
again**:

| arm | at the wall (tip / finalized) | after one more deploy (tip / finalized) | the block at the wall's tip |
|---|---|---|---|
| R1 (bootstrap shape) | 55 / 51 | 59 / 55 | 55 → finalised |
| R2 (rotating) | 35 / 31 | 39 / 35 | 35 → finalised |
| R3 (validator-1) | 29 / 25 | 35 / 31 | 29 → finalised |

**What is not yet captured, said rather than implied.** No run has yet held a *deploy* in the band and
then rescued it in the same transcript. The stored arms show deploys do land there (`07af032ad` R2/R3,
`513e2192b` R3); the lag arms show the band is covered once the chain produces; the two together give the
conclusion, but the single-run demonstration needs a run whose slack is **3**, and the rigs built here
produced slack 4 in all three arms — the chain mints about as many heights after a deploy as the band
is wide, which is exactly why the stored runs sat on the knife edge. A rig that catches it needs the
probe to run repeatedly until one arm lands at 3, or a way to stop production sooner after the last
deploy.

## Limits

- **One host, 3 validators, 8 GiB cap, one tree.** The band was measured at N=3; whether it is 4 at
  N=5 or N=8 is unmeasured, and `LIVENESS_WINDOW` is 5 in the code regardless.
- **The lag arms are a different rig from the rotation arms** (`n214-tail-lag-run.sh`, committed with
  this section). Their configuration is identical apart from the probe deploy.
- **Not a rate.** Four runs, twelve arms, three of them fresh. It is enough to say the spread is slack
  and not randomness; it is not enough to put a distribution on the slack.
