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
