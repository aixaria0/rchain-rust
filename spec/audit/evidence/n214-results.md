# n214 — results: criteria 1 and 2 re-probed on the tip

Protocol: [`n214-preregistration.md`](n214-preregistration.md). Run: `n214-sweep-run.sh`.
Artefacts: `n214-blocks/f9d36b9c4-20261003T154406Z/` (transcripts), `n214-repeat-attestation-on/`
(the void first attempt, kept — see below).

**Tree `f9d36b9c4`** (`dev`, the merge of PR #217). Image `sha256:eb2c2308…`, node binary
`sha256:9659a2bc…`, recorded in the sampler header. Rig: `tools/devnet.sh up --validators 3 --fresh
--stakes 100,100,50 --epoch-length 10 --no-autopropose --propose-on-deploy`, plus
`DEVNET_EXTRA_FLAGS=--attest-on-new-blocks`.

## The reading

**Arm A — criterion 1 (two marks, all three validators live throughout).**

| | |
|---|---|
| blocks first seen after deploy #1 | **6** (block-hash union) |
| blocks first seen after deploy #2 | **1** |
| deploy #1's block number | 1 |
| deploy #2's block number | 3 |
| **time to finality, deploy #1** | **never** |
| **time to finality, deploy #2** | **never** |
| every node alive at the end | yes |
| heights over the window | 1 → 4 |
| **verdict** | **fail** |

Production is **bounded** — 6 blocks, then 1 — but **nothing finalises**. Across 693 samples of the
three-node `finalized` column, the value is `none` in every one; there is no numeric reading at all.
So the deploy's block is proposed and never finalised, and the criterion fails on exactly the half the
issue says must decide it: *the witness is the deploy finalising, never "the height stopped moving".*

**Arm B — criterion 2 (kill one of three at 100/100/50).**

| | |
|---|---|
| node stopped | `v2` (the 50-stake validator) |
| survivors | `bootstrap`, `v1`; both alive through the kill window |
| finality at the kill | **none** |
| finality in the 120 s kill window | **none** |
| finality in the 90 s recovery window | **none** |
| **verdict** | **fail** |

The kill experiment cannot discriminate here, and that is itself the result: **there was no finality to
lose before the kill.** The net was already not finalising with all three validators live, so "do the
survivors finalise past the kill" has no meaningful answer — the pre-condition of the experiment is
false. Criterion 2 fails, but it fails upstream of the wedge.

## The first attempt was a repeat, not a void — a correction

The first run of this rig omitted `--attest-on-new-blocks` and produced the same shape (blocks, no
finality). This section originally called it **void** and attributed that to the omission, on the reading
that the positive clap flag defaults **false**.

**The reading was wrong, and the correction is #219's author's (`d960a0f18`).** The merged default is
`true` (`node/src/configuration/defaults.conf:16`) and the CLI flag only ever *adds* `true`, so the flag
was in effect in **both** runs — which is exactly why they gave identical results. The option's own doc
string, *"Attestation is on by default"*, was right all along.

So the first attempt is a **second, agreeing attempt**, not a void one. Its artefacts are kept under
`n214-repeat-attestation-on/` (renamed from `n214-void-attestation-off/`) so the name says what they are.

## What this does to the criteria

- **Criterion 1 is `fail`, with a committed artefact** — no longer 🟨 *reported, artefact absent*. The
  blocking fact is not the instrument (a height-delta confusion would make the *count* wrong, not make
  finality vanish) and not the tree. **The deploy does not finalise.**
- **Criterion 2 is `fail`, and the wedge does not need to be reached to say so.** This run does not
  reproduce #213's specific wedge (survivors producing one block each and then stopping): here
  production continued and finality never began.
- **#214's N=3 pass does not reproduce.** That issue records criterion 1 *passing* at N=3 on
  `0c6c65979` — "finality reached 3". On `f9d36b9c4`, with the configuration the issue names, no sample
  in either arm reports a finalised block, and the heights advance to 4. This is a direct
  contradiction and it is **unresolved**: either the configuration differs in a way not yet identified,
  or the behaviour has changed between the two trees. It should be settled before either reading is
  relied on, and it is the single most load-bearing open question this run produces.
- It is independent corroboration of the #149 result (*finality never advances at N ≥ 3*) and of the
  controlled A/B on #213 (*"finality's stall is independent of the escape"*): three separate
  measurements, three trees, the same shape.

## Limits, stated rather than implied

- One attempt per arm, one tree, one host. A single attempt is not a rate.
- N=3 only. A1.2 (N=5) and A1.3 (N=8) stay ⬜.
- **No bond was attempted**, so A2.4 (a new validator joins a running net) remains ⬜.
- `deploy_block_after` picks the first block with `deploy_count ≥ 1` seen after the deploy instant. With
  `--propose-on-deploy` and no `--autopropose` the only blocks built are deploy-triggered, but a system
  deploy in the same window would satisfy the same test. Stated in the summariser, not hidden here.
- Nothing here measures safety (TE-2).

---

# Re-run, 2026-10-04 (tree `ee1e204b3`) — arm A passes; **arm B is not a criterion-2 reading**

Artefacts `n214-blocks/ee1e204b3-20261004T151201Z/`, image `f18c0071…`. The image was rebuilt from the
tree by this run: the Dockerfile copies the sources with `target/` in `.dockerignore`, so a source change
busts the layer cache, and the digest differs from the earlier run's. This ran **after** C209/C210 and
C211–C214.

| arm | reading |
|---|---|
| **A — criterion 1, two marks** | **pass.** deploy #1's block finalises at block 1, 5 s; deploy #2's at block 6, 4 s; heights reach 15 with finality 11 |
| **B — kill/restart** | `fail` — **and the failure is the arm's, not the chain's** |

## Arm B cannot discriminate, and its own series says why

The arm deploys once at `t0+60 s`, kills validator-2 at `t0+90 s`, restarts it at `t0+210 s`, and ends at
`t0+300 s`. **It sends nothing inside its own window.** On a `--no-autopropose --propose-on-deploy` net
nothing is produced without a deploy, so once the pre-kill deploy's blocks are in, the chain is idle — and
idle means *finality cannot advance*, whatever the code does. The arm's own witness (finality "strictly
greater" after the kill) therefore fails on a **healthy** chain for exactly the reason it would fail on a
**stalled** one. It cannot tell them apart, and a reading that cannot distinguish the defect from the
healthy case is not evidence about either.

The series shows it in one line: at the kill the chain is at **height 6, finality 2**, and the pair does
not move again all arm — `6/2` at the kill, `6/2` through the 120 s window, `6/2` through the 90 s
recovery. Arm A, with the same rig and a second deploy, reaches `15/11`. There was simply no new block for
finality to advance onto.

**So this `fail` is not a criterion-2 reading — and neither was §3.4's.** That section reported the
re-probe as failing **both** criteria. Arm A's failure there was the real reading (nothing finalised at
all on the pre-fix tree); arm B's carried the same non-discrimination it carries here, and §3.4 now says
so. Nothing in the page's criterion-2 status rested on arm B either way: the ✅s come from `n213-run.sh`
(which *does* deploy inside the kill window) and the ⬜ from the live net.

**The fix, owed.** Put a driver inside the window — one deploy at `kill + 30 s`, one after the restart —
so the arm measures whether the survivors *can* produce and finalise while a validator is down, and
whether the returner takes part. That is a change to a **pre-registered** arm, so it needs its own
pre-registration rather than an edit to `n214-preregistration.md`.

## What arm A adds

A fresh confirmation of criterion 1's two marks on a tree carrying every fix since: the deploy's block
finalises in seconds, twice, with `--autopropose` absent. It is the reading §3.1 records, one tree later.
