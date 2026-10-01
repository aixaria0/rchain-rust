# Does the quotient remove the ramp? — round 2, under load — pre-registered

**Status: FROZEN before the run.**

**Why there is a round 2, in one paragraph.** Round 1 ran the frozen no-load rig on both trees and reached
a widest merge scope of **9 chains on every attempt on both arms**, with every peak at 49–66 MiB — 45×
below the threshold (`n127-directed-results.md`). The ramp's input is absent, so round 1 could not decide
and said so. The scope the ramp was measured at is not something this rig can produce; the rig that
reached it (`n127-campaign-run.sh`, 32–43 chains, 1.56 M states on this lineage) does so **under a load
this one does not apply**: four deploys at T+30 and a `stop 2` at T+120. Round 2 adds exactly that load to
exactly this measurement, on both arms, and nothing else.

Nothing below is a result. Acceptance rows were written before any arm ran.

## The rig, and what is and is not carried over

`n127-loaded-run.sh`: round 1's measurement (`n117-after-fix-run.sh`'s peak-sampling loop, its clean-stop
endpoint, its census extraction) with round 1's gaps filled:

| carried from round 1, unchanged | added from the census rig |
|---|---|
| `--validators 3 --stakes 100,100,50 --epoch-length 10 --fresh` | four bounded deploys of `examples/hello.rho` at **T+30** |
| 8 GiB cgroup, swap off | `tools/devnet.sh stop 2` at **T+120** |
| 3000 MiB threshold, clean stop on crossing | the bounded-deploy accounting (`submitted`/`timed_out`, 45 s bound) |
| 300 s window, N=3, one devnet at a time | — |
| the `merge search:` census line, per node, per attempt | — |

**The load is a perturbation, not a knob**, and the two offsets are absolute from each attempt's zero, as
in the campaign rig, so an attempt that stalls at a deploy cannot move the kill later and make itself
incomparable with the next.

**VOID, not negative:** fewer than three containers up; or an attempt whose widest scope stays below 20
chains, which is the width below which the two arms' units have not separated enough to compare.

## The arms and their acceptance rows, frozen

### The widths first — they gate everything else

| observation | verdict |
|---|---|
| the **control** reaches a widest scope ≥ 20 chains | the arms are comparable, and the census rows below are read |
| the control stays below 20 chains | **VOID for the census rows**; the widths are reported and the round says it could not carry the comparison |

### Arm A — the control, `6eacc4969`

| observation | verdict |
|---|---|
| ≥1 of 3 attempts has a node cross 3000 MiB | the rig reproduces the ramp on the pre-quotient tree, so arm B's crossing count is a comparison |
| 0 of 3 cross, with a widest scope ≥ 20 chains | the ramp does **not** reproduce even under load — reported as such, and the close condition's second conjunct is then **unmeetable on this host**, which is a finding about the rig and not about the fix |

### Arm B — the fixed tree, `865e8137e`

| observation | verdict |
|---|---|
| 0 of 3 cross while the control crossed | **the ramp is gone**, and the close condition's second conjunct is met end to end |
| ≥1 attempt crosses | the ramp survives the quotient; the census line of the crossing node is reported beside it and the close condition is **not** met by this |
| 0 of 3 cross while the control also crossed 0 | the memory half decides nothing; the census half below still carries |

### The census half, which is the close condition's own currency

The condition's first conjunct is *expanded states bounded by a function of the output*, and the census is
literally that measurement on a live node.

| observation | verdict |
|---|---|
| at a comparable widest scope, the fixed arm's `most states expanded` is ≥100× below the control's | the cost is output-bounded end to end, which is the conjunct |
| the fixed arm's `most states expanded` grows ~2× per chain as the scope widens | the quotient is **not** what the node is running; report it and stop |

**The unit caveat, restated because it decides how the ratio may be read.** The two arms count different
states — reachable accepted subsets against distinct rejection unions — so the ratio is the quotient's own
effect rather than a measurement error, and it is expected to widen with the scope, not to hold constant.
Reading a *fixed arm* number as "the cost" and comparing it with a historical figure from the other unit is
the mistake this paragraph exists to prevent.

### Arm C — #141's block-index cap

Read from the same arm-B runs, as in round 1: `index cache N entries … cap 64, E capacity evicted` and the
replay-fallback count. Round 1 read `E = 0` in 656 readings at 19–20 entries. A loaded run is the first
that can make the cap bind; if it still reads 0 under load, the cap is inert on this shape and the row says
so rather than implying 64 is calibrated.

## What this does not settle

- **C182's distribution and C184's `N`.** Still owed on C171's arm (`--no-autopropose
  --propose-on-deploy`), which is a different configuration from both rounds.
- Three attempts per arm on one machine is the measurement the register asks for before a claim is written
  down, not a proof.
- It does not measure an attacker: the parent set is item 2 of #127's change order and no rig here
  exercises it.
