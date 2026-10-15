# The read path's setup cost, attributed and removed (issue #364)

**One build, one box, several chain heights** — because every cross-section taken from outside the box
before this was three builds on two machines with two different read outcomes (`EMPTY expr` against
`values`), and nothing in it was attributable. Each sample below is one run of
`tools/probe-read-latency.sh` (`--samples 9`, read arm + no-op control) followed by one block and one
`/metrics` scrape — the scrape has to come *after* a block, because the census is published from the
DAG's observability tick and an idle node shows the counters frozen at boot.

## What the census says, per read

`rchain_runtime_read_*`, the casper census (`casper/src/runtime_manager.rs::read_path_census`). The
stage means are over reads only: they are recorded in `capture_results` and `fork_play_runtime_with_mode`,
which no other caller reaches. The decomposition closes exactly, which is how the attribution is
checked: `total = fork + reset + evaluate + replies`.

| height | read total | fork | reset | evaluate | probe result |
|---|---|---|---|---|---|
| 61 (before) | 10.29 ms | 2.94 | **6.37** | 0.97 | `EMPTY expr` |
| 200 (before) | 10.86 ms | 3.07 | **6.73** | 1.05 | `EMPTY expr` |
| 403 (before) | 10.93 ms | 3.02 | **6.81** | 1.08 | `EMPTY expr` |
| 61 (after) | **4.32 ms** | 3.32 | **0.00** | 0.99 | `values` |

Reproduce with `target/sweep-read-path.sh <label> [blocks]` (the driver is kept with the run, not in
`tools/`, because it is this measurement's instrument rather than the tree's).

## What it establishes

1. **The cost is flat in chain depth.** 10.29 → 10.86 → 10.93 ms across heights 61 → 200 → 403, i.e.
   0.0019 ms/block. The ≈0.13 ms/block fitted earlier was a mixed cross-section, and the one measurement
   that showed a slope (a 29.88 ms reset at height 201) was taken immediately after a 400-block bench,
   so it was measuring contention rather than depth. Law 68 states the same thing a priori: a read is a
   function of the version it names, and height is not an argument of that function.

2. **`reset` was two thirds of a read, and it was doing nothing.** `RhoRuntime::reset` replaced a store
   already at the root the fork named — and replaced it with an equivalent one — then re-installed the
   31 system-process definitions that `build_runtime_core` had already installed, so one read paid 62
   definitions to keep 31. Law 77 (`spec/Rchain/Reset.lean`) is the refutation: a reset to the root a
   view already names is a projection, so the first pass was unobservable. The one thing the reset did
   that the fork did not was install the native reader, so the fix moves that to the fork
   (`RSpace::with_native_store`) and drops the reset.

3. **The read's answer is unchanged**, which is what the law predicts and what the probe confirms: the
   probe's term is `pos!("getBonds")`, a *native* read, and it returns `values` after the fix where the
   pre-fix build on these nodes returned `EMPTY expr` for other reasons (the two public nodes' older
   builds) — and `values` here in both arms' terms. The witness test
   (`casper/tests/exploratory_reply.rs::a_read_makes_no_reset`) pins the shape: a read performs no
   reset, and still installs the whole definition list at least once.

**What it does not establish.** Nothing here explains the ~318 ms the public playground pays; on this
build a read is ~4 ms node-side at height 61. That gap is a build or a box, not a depth and not this
path, and issue #364 keeps it open.

## Files

- `probe-*.txt` — the probe's two arms, the delta, and the network breakdown
- `census-*.txt` — the `rchain_runtime_read_*` gauges at that sample
- `metrics-*.txt` — the full scrape, for any gauge this summary did not name
