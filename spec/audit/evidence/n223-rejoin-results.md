# n223 — a validator that left and returned rejoins: results

Rig: [`n223-rejoin-run.sh`](n223-rejoin-run.sh). Artefacts: `n223-rejoin-blocks/`. Pass record:
[`passes.md` §65](../passes.md).

Net: 3 validators at 100/100/50, `--no-autopropose --propose-on-deploy`, `--epoch-length 10`. The rig
stops `devnet-validator-2` (the 50), sends six deploys to the survivors 12 s apart, restarts it, waits
for catch-up, then sends a deploy **to the returner**.

| | condition | how it is read |
|---|---|---|
| R1 | the returner reaches the survivors' tip | its `/api/blocks` height equals theirs |
| R2 | a deploy sent to the returner afterwards finalises | the deploy's own status is `ProcessedWithSuccess` and its block's `/api/is-finalized` is true |
| R3 | nothing is refused | no `missing justification` line in the returner's log |

## Baseline — `dev` at `f1ca009`, unfixed (`baseline-dev/`)

**#223 reproduced.** Survivors at 34, the returner stuck at **14** three minutes after restart and at 15
after its own deploy, with two `block summary failed: missing justification` refusals
(`baseline-dev/refusals.txt`). R1, R2 and R3 fail.

## Fixed — the working tree carrying C211–C214 (`fixed-run1/`, `fixed-run2/`)

Binary built natively from the branch, image `sha256:eb79cdfb…`. The manifests read `tree=f1ca009`
because the run preceded the commit; the image is the fix.

**Every condition passes, twice.**

| run | survivors at restart | returner after catch-up | after the returner's deploy | R1 | R2 | R3 |
|---|---|---|---|---|---|---|
| `fixed-run1` | 55 | 55 (22 s) | 59, finalised 55 | PASS | PASS | PASS |
| `fixed-run2` | 55 | 55 (21 s) | 59, finalised 55 | PASS | PASS | PASS |

The deploy sent to the returner landed in a finalised block (`d65f4dc2…` in run 2). During the absence
each deploy cost about ten heights on the two survivors; with all three live, about four.

## Regressions — the earlier live arms on the same image (`regressions/`)

| arm | rig | result |
|---|---|---|
| rotation (#214) | `n214-rotation-run.sh`: R1 all to bootstrap, R2 rotating, R3 all to validator-1 | each arm: 6 of 6 deploy blocks at or below the final fringe (highest 45 ≤ 51, 29 ≤ 31, 25 ≤ 25); no block in the 90 s read |
| kill (#213) | `n213-run.sh` | **PASS**: baseline, absent-deploy and post-restart deploys all finalised, case (d) within budget |
| join (#220) | `n220-join-run.sh` | J1, J2, J3, J4a **PASS**; J4b reported **FAIL**, height 40 → 45 over the read |

**The J4b FAIL is the read window, not a storm, and is recorded as the rig reported it.** The rig takes
its first height at 11:54:08, as J3 confirmed the deploy finalised; the blocks still in flight
ended with block 44 on every node at 11:54:12.3–12.7
(`regressions/join/*.log.gz`), and no node proposed in the 86 s after. On #219's image the same rig read
24 → 24; that the difference is C214's extra round is inferred, not measured.

## Limits

One host, no delivery delay, a returner at 50 of 250 (its absence never cost the survivors the
supermajority). Not measured: a returner whose absence breaks the supermajority (a 100 at 100/100/50),
a returner that missed an epoch boundary carrying a bond change, or the public testnet's stake shape.
C215 (two nodes merging the same justifications to different pre-states) was seen only with C212 and
without C213, and is not explained.
