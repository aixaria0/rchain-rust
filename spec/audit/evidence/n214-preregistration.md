# n214 — pre-registration: criteria 1 and 2, re-probed on the tip

**Frozen before the run.** Issue: [#214](https://github.com/rchain-community/rchain-rust/issues/214).
Criterion 2's home issue: [#213](https://github.com/rchain-community/rchain-rust/issues/213).

This run exists because the first edition of `docs/src/spec/testnet-acceptance.md` measured the tree
`1e5a64ed4` from a clone that had never been fetched, and so missed `#215` — the landed fix for the wedge
in criterion 2 — as well as the tree `0c6c65979` that criterion 1's pass cites (see the page's §0.9
corrigendum). It re-probes both criteria against the current tip, with two changes to the protocol the
original audit did not make:

1. criterion 1 is measured with **two marks**, not one, because a single deploy's bounded production is
   satisfied *by construction* by a chain that has wedged — the failure criterion 2 measures;
2. the "blocks per deploy" number is taken from the **block-hash union**, not from `latestBlockNumber`,
   because a block's number is derived from its justification set and several blocks share a height.

## Rig

```
tools/devnet.sh build                                     # rnode:local from the recorded tree
DEVNET_EXTRA_FLAGS=--attest-on-new-blocks \
tools/devnet.sh up --validators 3 --stakes 100,100,50 --fresh \
    --no-autopropose --propose-on-deploy --epoch-length 10
```

> **Corrected 2026-10-04 — this claim was wrong, and the correction is a peer's.** This section originally
> read *"`--attest-on-new-blocks` is required, and its absence is a rig defect"*, on the reading that the
> positive flag is a clap `bool` defaulting **false**. It is not: the merged default is **true**
> (`node/src/configuration/defaults.conf:16`), and the CLI flag only ever *adds* `true`. Passing it is
> **optional**, and its absence cannot stop a node attesting. The option's doc comment — *"Attestation is
> on by default"* — was right all along. The correction is the author of #219's (`d960a0f18`).
>
> **What that means for the first attempt.** It is **not** void. It is a second, agreeing attempt: both
> runs gave identical results *because* the flag was in effect in both. Its artefacts are kept under
> `n214-repeat-attestation-on/` (renamed from `n214-void-attestation-off/`), a name that misstated what
> they are, and the reader should treat that directory as a repeat rather than a failure. The frozen text is left above the correction because this
> tree's rule is that a superseded claim stands beside it.

The node still gates the attestation tap on `conf.attest_on_new_blocks &&`
`!conf.no_attest_on_new_blocks` (`node/src/runtime/node_runtime.rs`); what the gate's *effective* default
is depends on the config file, not on the flag's absence. #213's and #214's configurations both carry the
flag, as do this rig's — explicitly, so the argv is self-describing.

Node argv is recorded, not paraphrased: there is **no `--no-autopropose` flag on the node** — the flag
above is `devnet.sh`'s, and it *omits* `--autopropose`. `--stakes 100,100,50` gives the three validators
100/100/50, so killing the third leaves 100+100 of 250 stake — 80 %, a supermajority on the whole bonded
map, which is the arrangement #213 measured. (`devnet.sh --stakes 100,100,100` is refused precisely
because three equal validators minus one is exactly ⅔; the uneven split is the point.)

`--fresh` starts from genesis. The tree, image id and node binary sha256 are recorded in the sampler's
`series.tsv` header. A run whose sampler header does not name the intended tree is **void**.

## Arm A — criterion 1, two marks

Sampler covers the whole arm. `t0` is the instant all three nodes report `latestBlockNumber > 0`.

| mark | at | what happens |
|---|---|---|
| `deploy1` | `t0 + 60 s` | `tools/devnet.sh deploy <a simple contract>`, exactly one |
| `fin1` | ≤ `deploy1 + 90 s` | the earliest sample with `finalized ≥` the deploy block's height |
| `deploy2` | `deploy1 + 90 s` | a second deploy, exactly one |
| `fin2` | ≤ `deploy2 + 90 s` | the earliest sample with `finalized ≥` the second deploy's block height |
| `end` | `deploy2 + 90 s` | sampler stops |

**Witness (a pass requires the positive observation, not the absence of one).** Both deployments must
**finalise** — `last-finalized-block ≥` their block heights — with all three validators alive throughout.

**Falsifier.** If `fin2` is never observed while the survivors are alive and producing, the criterion
**fails**: production stopped being driven by the deploy, which is the defect, not the pass. If blocks
flow past `fin2` with no bound, the criterion fails as unbounded.

**Reported quantities.** `blocks(deploy1)` and `blocks(deploy2)` = the count of *distinct block hashes*
first seen in each window, from the union — never a height delta. `time_to_finality` per mark, in seconds.

## Arm B — criterion 2, kill / restart

Same rig, fresh genesis. Sampler covers the whole arm.

| mark | at | what happens |
|---|---|---|
| `t0` | — | all three past genesis |
| `settle` | `t0 + 60 s` | a pre-kill deploy, so the pool is non-empty when the kill lands |
| `kill` | `t0 + 90 s` | `tools/devnet.sh stop validator-2` (the 50-stake validator) |
| `kill_end` | `kill + 120 s` | the kill window closes |
| `restart` | `kill + 120 s` | `tools/devnet.sh start validator-2` |
| `recover_end` | `restart + 90 s` | the recovery window closes |

**Witness.** (a) at least one sample after `kill` shows the two survivors' `finalized` **strictly greater**
than it was at the `kill` mark, with both survivors alive; (b) after `restart`, at least one sample shows
`finalized` advancing again with three nodes alive.

**Falsifier.** If both survivors stay alive and report a `finalized` that does not move for the whole
120 s kill window, criterion 2(a) **fails** — and the run reproduces #213. If it resumes only after an
operator action beyond `start` (a reset, a re-genesis), 2(b) fails.

## What this run does **not** measure, and says so

- **N=5 and N=8.** A1.2 and A1.3 stay ⬜. This rig is three validators; the sweep budget is not a sweep.
- **A new validator bonding onto a running net (A2.4).** A bond takes effect on a merge; that path is only
  reachable once (a) passes, and this run does not attempt a bond.
- **Safety (TE-2).** Nothing here measures whether two validators can each finalise a conflicting block.

## Void conditions

A run is void, and its readings are not used, if any of: fewer than 3 nodes report `latestBlockNumber > 0`
within 180 s; the sampler header's tree is not the intended tree; a deploy is refused (`deploy` exits
non-zero) — a window with no driver measures nothing; or a node is OOM-killed rather than stopped
deliberately (the sampler marks a node `alive=0`, and `stop` and an OOM are indistinguishable from the
sampler's side, so the run script records which node it stopped and when, in `marks.tsv`).

## Instruments

- `spec/audit/evidence/n149-sample.py` — **shared, unchanged**. It samples `height`, `finalized`, `alive`
  per node and maintains the block-hash union `blocks.tsv`. Reused rather than re-implemented: a second
  sampler would be a second implementation of one reading.
- `spec/audit/evidence/n214-summarise.py` — new; the reduction for **these** questions (two marks,
  kill/restart), reading the same artefacts. It computes nothing the sampler did not record.

## Outcome vocabulary

`pass` / `fail` / `void`, per arm, against the witnesses above — and the tree is named in the result, so
a later reader can tell which build it speaks for.
