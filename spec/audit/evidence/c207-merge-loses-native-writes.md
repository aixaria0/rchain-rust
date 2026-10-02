# C207: a user deploy's native write is lost when its block is merged

Measured 2026-10-02/03 on tree `738fe96ed` (branch `feat/delegation-read`), two- and one-validator
devnets, the read taken with `pos!("getDelegations", …)` — a **typed** probe, computed in rholang, so no
rendering is involved (see `n203-native-write-probe.md` §3 for why that mattered).

## 1. The finding

**`delegate` writes the delegation ledger and the operator's aggregate; on a chain that merges, both are
gone within a few blocks.** On a single-branch chain the same deploy survives indefinitely.

The ablation, each row a separate run from `down -v`:

| configuration | the read afterwards | verdict |
|---|---|---|
| 2 validators, `--no-autopropose`, epoch 2 (boundaries every 2 blocks) | `GInt(40)` at **block 54** | survives |
| 2 validators, `--no-autopropose`, default epoch (no boundary in reach) | `GInt(40)` | survives |
| 2 validators, **autopropose**, epoch 2 | `ps: []` | **lost** |
| 2 validators, **autopropose**, default epoch — **no boundary at all** | `ps: []` | **lost** |
| **1 validator**, autopropose (`trust` instead of `delegate`, since one validator cannot delegate to itself) | `GBool(true)` | survives |

Two things follow, and they are the reason to trust the table rather than the earlier episode:

- **The boundary is exonerated.** The loss reproduces with the default epoch length, where no
  `close_block` runs. The earlier attempt to blame `--epoch-length 2` was wrong: that configuration has
  boundaries *and* fast blocks.
- **The trigger is concurrent block production.** `--no-autopropose` produces one block per deploy and a
  linear chain; autopropose on two validators produces the DAG that merges. The one-validator run —
  autopropose on, but no second proposer and so no concurrent branch — survives.

The failing configuration logs it directly:

```
merge search: 318 merges · widest scope 6 chains / 18 conflict pairs / 12 asymmetric ·
              most states expanded on one merge 6
```

## 2. Why this is the phenomenon #203 chased, and why that episode still closed correctly

**C205** claimed a user deploy's native write is invisible to a later deploy's read. It was **void** as
written: the reading rested on grepping `listen-data-at-name`'s derived `Debug` for hex, on a surface
that renders a 65-byte key as `GByteArray([2, 2, …])` — decimal. Every run behind it used
`--no-autopropose`… except the *first* one, which is why the `delegate`→`getBonds` reading of `100` that
started all of this was **right about a real loss and wrong about its cause**.

So the register's record stands and this is a new row: C205's closure is about *that measurement*, and
C207 is the defect it was reaching for, measured with an instrument that can see it.

## 3. What it means

`bond`, `withdraw`, `trust` and `delegate` all write native state from an ordinary user deploy. If a
merge drops those writes, then on any network with more than one proposer — that is, any real network —
**none of them takes effect**. The register recorded this class as closed: #74 ("Post-genesis validator
admission does not take effect") was closed by the `NativeChangesStore` sidecar, and C205's retraction
removed the row that said it had come back.

The mechanism is the sidecar the #74 fix introduced, or the merge's application of it. **Not measured
here**, and the reason to say so: `merge` re-applies a block's native changes from
`load_native_changes(post_state_hash, sender, seq_num)`, falling back to `regenerate_sidecars` when the
entry is absent; the play path saves that entry from `last_native_changes` *accumulated across its
checkpoints* while the replay path saves it from a single checkpoint. Which of those is missing a
**user** deploy's writes is the next measurable question, and the harness is in place:
`casper/tests/deploy_native_write.rs` plays the op through `compute_state` and passes, so the extension
is to drive it through a merge.

## 4. What it costs #193

The delegation primitive **cannot work on a multi-validator network** until this is fixed, which makes
C204's live arm un-runnable for the same reason — and it means the arm's earlier failure was not the
instrument's fault after all, only its *reading* was.
