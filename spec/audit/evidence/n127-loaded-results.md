# #127's controlled baseline, round 2: under load the quotient widens the scope sixfold and cuts the cost by three to four orders of magnitude

Protocol: `n127-loaded-preregistration.md` (frozen before the run).
Rig: `n127-loaded-run.sh`. Both arms 3 attempts, 8 GiB, 3000 MiB threshold, 300 s window, four bounded
deploys at T+30, `stop 2` at T+120.

**Result: the load is what round 1 was missing.** On the pre-quotient tree the widest merge scope is
**29 chains** and the worst single merge expands **615,599–650,159 states**. On the fixed tree the widest
scope is **161–174 chains** — five to six times wider — and the worst merge expands **153–290 states**.
Every merge on the fixed arm sits in the cheapest bucket; the control put nine samples in the 10⁵ bucket.

**The memory ramp itself still does not reproduce**, on either arm: no node crossed 3000 MiB. The peaks are
the finding instead — **647–689 MiB on the control against 31–45 MiB on the fixed arm**, while the fixed
arm is carrying the *wider* DAG.

## The two arms, per attempt

| arm | tree | attempt | widest scope | conflict pairs | asymmetric | **worst merge, states** | peaks (b / v1 / v2, MiB) | crossed |
|---|---|---|---|---|---|---|---|---|
| control | `6eacc4969` | 1 | 29 | 455 | 324 | **650,159** | 689 / 688 / 31 | 0 of 3 |
| | | 2 | 29 | 455 | 324 | **650,159** | 686 / 684 / 36 | 0 of 3 |
| | | 3 | 29 | 456 | 323 | **615,599** | 647 / 649 / 34 | 0 of 3 |
| fixed | `865e8137e` | 1 | **161** | 12,818 | 12,586 | **290** | 42 / 44 / 35 | 0 of 3 |
| | | 2 | **169** | 14,287 | 14,043 | **160** | 40 / 38 / 34 | 0 of 3 |
| | | 3 | **174** | 15,781 | 14,091 | **153** | 45 / 41 / 31 | 0 of 3 |

Two things in that table are worth more than the headline:

- **The control is reproducible to the state count.** Attempts 1 and 2 report *identical* envelopes —
  29 chains / 455 pairs / 324 asymmetric / **650,159** states — from three nodes that computed them
  independently, and attempt 3 differs only where the fork did. That is the census's determinism claim
  holding on a real fork, and it is what makes the comparison a measurement rather than a sample.
- **The fixed arm's scopes are not rescaled versions of the control's.** 161–174 chains and 12,818–15,781
  conflict pairs are widths the enumeration cannot reach at all: the quotient is not merely cheaper on the
  same input, it is running *past* where the old one stops. The `width buckets` show it — 50–67 samples
  above 128 chains on the fixed arm, where the control has none above 32.

## Against the frozen rows

| row | outcome |
|---|---|
| widths ≥ 20 chains on the control | **met** — 29 chains on all three attempts, so the census rows are read |
| Arm A: the control crosses | **not met**, 0 of 3 — so per the row's second branch the memory half decides nothing, and that is a finding about the rig, not about the fix |
| Arm B: 0 of 3 crossings | read with the control's 0, this decides nothing about the ramp |
| **census: ≥100× below the control at a comparable scope** | **met** — the fixed arm's worst merge is ≤290 against the control's 615,599 at a *narrower* scope, i.e. ≥2,122× below, and both arms' growth with width is what separates them (the control is `2^n`-shaped, the fixed arm is flat in the cheapest bucket across a 6× width range) |

**So the close condition's first conjunct — expanded states bounded by a function of the output rather
than of the conflict-set width — is met end to end on a live network.** The second conjunct is not: there
is no OOM ramp on either arm to be gone, and no rig in this repository has produced one since the proposer
fix moved the DAG's shape. That is recorded as the residual rather than smoothed over.

## Arm C — #141's block-index cap binds under load, and a third outcome

| arm | blocks indexed | replay fallbacks | index cache | pruned | evicted |
|---|---|---|---|---|---|
| control | 14,500 | 1 (624 ms) | 19–20 entries | 369–375 | — (pre-#141, no such counter) |
| fixed | 32,750 | 1 (721 ms) | **64 entries (= the cap)** | 354 | **16,288** |

The preregistered row offered two outcomes and the run produced **a third**, which is stated rather than
forced into one of them:

- the cap **does** bind under load — 16,288 capacity evictions at scopes up to 174 chains, where the cache
  holds 64 of the ~170 indices a merge wants;
- and the one cost metric available to this rig **does not move**: exactly **1** replay fallback on *both*
  arms (624 ms against 721 ms), at 2.3× the indexed blocks on the fixed arm.

So the honest reading is: **the cap binds and costs nothing that this measurement can see.** What it cannot
see is the recomputation the eviction forces — an evicted index is rebuilt from stored deploy data, which
is CPU rather than a replay fallback, and this rig samples memory, not the merge path's time. The cap is
therefore neither vindicated nor convicted here; what the run adds is that 64 is small enough to bind on a
real forked DAG, which the cache's own comment did not know.

## What this does not settle

- **The OOM ramp.** No node crossed, on either arm, at 8 GiB. #117's cgroup ceiling is still unreproduced
  on the current lineage, and nothing here says whether the quotient would prevent it if it were — only
  that the search's share of the heap (the profile's 97 %) is now 2,000× smaller where it can be compared.
- **C182's distribution and C184's `N`.** Still owed on C171's arm (`--no-autopropose --propose-on-deploy`),
  a different configuration from both rounds.
- **Three attempts per arm on one machine** is the measurement the register asks for before a claim is
  written down, not a proof.
- The fixed arm's wider scopes are its own effect and are *not* evidence that a proposer would build such a
  DAG unattended: the rig's kill at T+120 creates the fork both arms then merge over.

## The artifacts

Per arm, under `n127-loaded/<arm>-<tree>/`: `run.log` (every printed peak, crossing and census line),
`manifest.txt`, the queue-depth samples, the deploy accounting, and per node per attempt a `census-*.txt`
and a `cache-*.txt` extract. The nine full node logs per arm are not committed; `full-log-digests.txt`
records their SHA-256 so the extracts can be checked against the raw artifacts in `target/n127-loaded/`.

**One disclosure.** The rig was edited once after the preregistration was frozen and before this arm ran:
`exec > >(tee "$OUT/run.log")`, so the peaks — which exist only on stdout — land in the arm directory
instead of being lost to a caller's pipe. The first attempt of the control arm was aborted two minutes in
for exactly that reason. The edit touches no experiment parameter: shape, cap, threshold, window, attempt
count and load offsets are the frozen ones, and the aborted attempt produced no reading.
