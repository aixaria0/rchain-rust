# Does the directed quotient remove the ramp? — pre-registered

**Status: FROZEN before the run.**

Issue **#127**. Its close condition, verbatim, is the falsifier this campaign exists to run:

> expanded states bounded by a function of the **output** (the rejection options) rather than of the
> conflict-set width — **and** the controlled baseline (`spec/audit/evidence/n117-after-fix-run.sh`, same
> cap and window as the pre-fix runs) showing the ramp is gone.

Nothing below is a result. Acceptance rows were written before any arm ran, and the arms are reported as
they come out — including as null.

## Why this is the measurement

C178's `owes`, verbatim, names both halves:

> the directed case, which is the measured one: enumerate the terminal states of a digraph … Then re-run
> the controlled baseline - the parent commit through `spec/audit/evidence/n117-after-fix-run.sh`, which now
> has its image recipe tracked beside it (`build-stats-image.sh`), so the baseline is runnable from the
> repository alone; **the pre-fix runs on record crossed at a different cap and window and cannot decide
> whether the ramp is gone**.

The directed case landed as **#141** (states quotiented by the rejected set, which is what the search
returns, so accepted sets sharing a rejection union collapse into one state) and is gated in process by
`rejection_options_are_bounded_on_a_directed_shape`. **This campaign is the other half:** the same script,
the same constants, run against the tree before the quotient and the tree after it.

**The two trees.**

| arm | tree | what it has |
|---|---|---|
| **A — the control** | `6eacc4969`, the parent of #141 | the C177 dedup, the symmetric-case rewrite, and the `SearchBudget` bound — but **not** the directed quotient, so the node's directed relation still takes the enumeration |
| **B — the fixed** | this branch's tip | the same, plus the quotient |

The control is the parent rather than "the pre-fix runs on record" because those crossed at **4 GiB** and a
different window; this script's own header records why 4 GiB cannot be used (`at 4 GiB the OOM-killer wins
the race against the clean stop — measured, 8 of 9 nodes died before it landed`). Running the *parent*
through the *same script at its own defaults* is what makes the pair comparable, and it is what the row
asks for.

**The rig is `n117-after-fix-run.sh`, unmodified** — `--validators 3 --stakes 100,100,50 --epoch-length 10
--fresh`, an 8 GiB cgroup with swap off, a 3000 MiB threshold, a 300 s window, N=3 attempts fixed in
advance and unfiltered, one devnet at a time. Only the image tag changes between arms: the script's
manifest line reads `docker inspect rnode:local`, so each arm is run with `rnode:local` pointing at that
arm's binary and the image id is recorded per arm rather than assumed.

**VOID, not negative:** fewer than three containers up, or an attempt that never reaches a wide merge
scope, gives no reading. The manifest records the census of each run and such an attempt is reported VOID.

## The instrument's unit changed between the arms, and this is preregistered because it is a trap

`merge search: … most states expanded` is counted in **accepted-subset states** on arm A and in **distinct
rejected sets** on arm B (`SearchCensus::expanded`; the arithmetic is in `casper/src/merging.rs`'s
`EXPANDED_EDGES` comment — on a dependency chain of `n` chains the old unit gives `2^n - 1` and the new one
`n`). So:

- the **crossing** is the quantity compared *across* arms, because it is a byte count read from the cgroup
  and not from the search;
- the **census** is read *within* each arm, as "did this node's search reach a large number on the unit
  its own tree uses". A smaller census on arm B is **not** evidence by itself — the unit moved under it.

## The arms and their acceptance rows, frozen

### Arm A — the control, `6eacc4969`

| observation | verdict |
|---|---|
| ≥1 of 3 attempts has a node cross 3000 MiB | **the rig reproduces on the pre-quotient tree**, so arm B's result is a comparison rather than a lone observation |
| 0 of 3 attempts cross | the rig is not reproducing, **so this campaign cannot decide whether the ramp is gone** — reported as such, and *not* as evidence for the fix |

### Arm B — the fixed tree

| observation | verdict |
|---|---|
| 0 of 3 attempts cross 3000 MiB | **the ramp is gone** on the same rig and constants the control ran at |
| ≥1 attempt crosses, and the crossing node's last census line reports a large `most states expanded` | the ramp survives the quotient, and the census names the search as the mechanism |
| ≥1 attempt crosses while every census line is small | the crossing is **not** this search; reported per node, and it does not close #127 |

**The single number to report, per arm:** each node's peak `anon` in MiB, how many of the 3 crossed, and the
largest `most states expanded` any node's census line reported.

### Arm C — the quotient's own cache bound, on the same runs

#141 also caps the block-index memo at 64 entries, and that half is his number and unmeasured. Read off the
same arm-B runs, from the node's own progress line: `index cache N entries … cap 64, E capacity evicted` and
the replay-fallback count.

| observation | verdict |
|---|---|
| `capacity evicted` stays 0, or grows slowly against a stable fallback count | the cap never binds on this shape; it costs nothing here, and that is all this run says |
| `capacity evicted` grows with the merge rate **and** replay fallbacks rise with it | the cap **thrashes** the merge path; the second half of #141 goes back as its own measured change |

## What this does not settle

- **C182's distribution and C184's `N`.** Both are owed on C171's arm (`--no-autopropose
  --propose-on-deploy`), which is a different configuration from this one and is run separately.
- **Three attempts on one machine is not a proof**, it is the measurement the register asks for before a
  claim is written down — the same caveat every #117/#127 campaign carries.
- It does not measure an attacker. The ramp here is the node's own operation; the attacker-controllable
  input is the parent set, which is item 2 of #127's change order and is not exercised by this rig.
