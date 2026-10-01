# #127's controlled baseline, round 1: the rig no longer reproduces the ramp, so it cannot decide

Protocol: `n127-directed-preregistration.md` (frozen before the run).
Rig: `n117-after-fix-run.sh`, unmodified, via `n127-directed-run.sh`. Both arms 3 attempts, 8 GiB, 3000 MiB
threshold, 300 s window, `--validators 3 --stakes 100,100,50 --epoch-length 10 --fresh`.

**Result: 0 of 3 crossings on both arms — including the control, which is the tree *before* the quotient.
By the pre-registered row that is the *"the rig is not reproducing"* outcome, and the campaign therefore
cannot decide whether the ramp is gone.** It is reported as that, and not as evidence for the fix.

## The two arms

| arm | tree | image | attempts | crossed | peaks (bootstrap / v1 / v2, MiB) |
|---|---|---|---|---|---|
| control | `6eacc4969` | `b3411e185fa1` | 3 | **0 of 3** | 60/49/55 · 64/65/64 · 61/66/63 |
| fixed | `865e8137e` | `46658753477f` | 3 | **0 of 3** | 64/63/62 · 63/61/64 · 61/59/63 |

No node on either arm came within **45×** of the threshold: the highest peak in the whole campaign is
66 MiB against a 3000 MiB threshold. There is no ramp to be gone.

## Why this is a null, and the census says why

Every attempt on both arms reports the same shape, and it is nothing like the shape the ramp was measured
at:

| arm | widest scope | conflict pairs | asymmetric | most states expanded on one merge |
|---|---|---|---|---|
| control | **9 chains** | 43 | 28 | 104 · 118 · 99 |
| fixed | **9 chains** | 43 | 27 | 8 · 13 · 12 |

The ramp's inputs are absent. `1,663,395` states needed 33–43 chains; this rig reaches **9**, on every
attempt, on both trees, and each attempt's three nodes agree exactly (which is the census's own
determinism check passing, not a reading).

**That is not the quotient's doing, and the control arm is what proves it**: the control does not have the
quotient and it does not ramp either. The scopes came down on the *lineage* — the proposer fix (#138) and
the DAG-shape work moved the node's own merges from 32–43 chains to the 9–31 band, and this rig, which
applies **no load at all** beyond the chain's own operation, sits at the bottom of it consistently. The
campaign that reached wide scopes under load is `n127-campaign-run.sh`'s, with four deploys and a kill;
this one has neither.

## What it does measure, in situ, at the width both arms reached

At the same widest scope — 9 chains, 43 conflict pairs — the two arms' own censuses read:

- control: **104 / 118 / 99** states expanded on the worst merge;
- fixed: **8 / 13 / 12**.

That is the quotient's effect on the live node, ~10× at nine chains. It is *not* a like-for-like ratio and
the preregistration says so before the fact: the two arms count different states (reachable accepted
subsets against distinct rejection unions), and the ratio widens as the scope does — the in-process gate
pins it at 16,383 against 14 at fourteen keys, and `2^40 - 1` against 40 at forty. What this reading adds
is that the quotient is live on the node and moving the counted work there, not only in a test.

## Arm C — #141's block-index cap, which never binds

Across **656** `index cache …` readings on arm B: `cap 64, 0 capacity evicted`, with the cache sitting at
**19–20 entries** and **930–942 pruned** by finality. Per the frozen row: the cap never binds on this
shape and costs nothing here — and that is all this run says about it. It is not evidence that 64 is right
for a node under a storm, which is the shape the cache's own comment (`casper/src/merging.rs`) is about.

## What this does not settle, and what it does not claim

- It does **not** show the ramp is gone. It shows the rig cannot produce the ramp, on either tree.
- It does **not** widen the margin between the arms beyond 9 chains. The exponential gap between the two
  units is pinned in process (`rejection_options_are_bounded_on_a_directed_shape`) and not here.
- **C182's distribution and C184's `N`** remain owed on C171's arm, which is a different configuration.

## Round 2, and why it is a second preregistration rather than an edit to this one

The ramp needs a scope this rig does not reach, and the rig that reached wide scopes is the campaign's,
whose load is named and bounded (four deploys at T+30, `stop 2` at T+120). Round 2 runs that shape through
the same peak/crossing measurement **on both arms**, pre-registered separately
(`n127-loaded-preregistration.md`) because "same script, same constants" is the only thing that made round
1 worth running — and adjusting a frozen rig after seeing its result is how a campaign stops being one.

## The artifacts, and what is deliberately not in the repository

Per arm, under `n127-directed/<arm>-<tree>/`: the rig's own `run.log` (the peaks, the crossings and the
census lines exactly as the run printed them), the `manifest.txt` with the two tree shas and the image id,
the three `queue-a*.tsv` samples, and one `census-<node>-a<n>.txt` per node per attempt.

The nine full node logs per arm are **not** committed: 7 MB each, and every number in this file comes from
one `grep 'merge search'` over one of them. To keep that extract from being an unverifiable subset,
`full-log-digests.txt` records the SHA-256 of each full log as it was when the extract was taken, so the
extracts can be checked against the raw artifacts in `target/n127-directed/` (gitignored) without carrying
them in the tree.
