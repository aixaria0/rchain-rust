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

## 5. The root cause, and the fix

**The native channel recorded *snapshots*; every other mergeable effect in this tree records a
*transition*.** The tuple space merges because the merge re-applies each accepted chain's deploy effects.
Native state had no such record — `NativeStoreState` carries absolute whole-slot values — and two
absolute snapshots of one slot cannot be composed, so the merge had exactly one recourse: **reject a whole
block**.

The one slot that made that universal is `pos:vault`. `pre_charge`, `refund` and `pay_executor` run for
**every user deploy** and all three write it, so every user-deploy block overlapped every concurrent
sibling on that slot, lost the resolution, and was rejected whole — taking the deploy's own writes (the
delegation ledger, the bond, the trust) with it. A **system** deploy escaped, and that asymmetry is why
nothing looked wrong: `close_block` writes no `pos:vault`, and two sibling boundaries compute *identical*
values from the same pre-state, so rejecting one leaves the other's equal write in place while the epoch
machinery keeps working.

**So cost accounting left the block's native set and is merged per accepted deploy.** The three moves are
pure balance movements that sum to zero, so unlike snapshots they *compose*; all three are functions of
the `ProcessedDeploy` the block already carries plus the host block's own signed `sender`, which is why
the merge can re-derive them rather than a sidecar carrying values:

| move | amount | input |
|---|---|---|
| `pre_charge` | `deploy.data.total_phlo_charge()` | the deploy data |
| `refund` | `processed.refund_amount()` | the `ProcessedDeploy` |
| `pay_executor` | `processed.burned_amount() × executor_share / 10000` | same, plus `PosParams` |
| the producer | the **host block's** signed `sender` | the block itself |

Provenance is exact rather than heuristic: the three system deploys are evaluated *outside* the deploy's
own reduction (`play_deploy_with_cost_accounting_once`), so a mode set around them on the
`InMemNativeStore` marks exactly their writes and nothing a user term can reach. A slot that anything else
writes stops being cost-only, so a `bond` debiting the deployer's own vault keeps its whole post-state
value in the sidecar.

**The residual, stated because it is not fixed and is not hidden:** two *concurrent* `delegate` blocks
still conflict on `pos:delegations`, and one is still rejected whole; `trust` (`pos:trusted`) and
`withdraw` (`pos:pending_withdrawers`) are the same shape. A ledger is a set, not a sum, and composing
sets is a separate design. What this fix removes is the one conflict that was *universal* — the one that
made the primitive a no-op for every deploy on every merging network rather than for a rare pair.

**And a second instance of the same shape, measured while running the arm rather than reasoned about
here:** two *boundary* blocks at one height still conflict, because their `close_block`s carry different
absolute snapshots of the same PoS leaves — the reward pots differ by what each block charged — so the
merge rejects one whole and a deploy riding the loser goes with it. It is pre-existing and deliberate
(`casper/src/merging.rs::boundary_merge_tests::sibling_boundaries_with_different_pots_merge` asserts
exactly this resolution, and predates C207). Measured at `epoch-length 2` with both validators proposing
on every gossiped deploy: `delegate` landed in block 206 and every later read reported `getDelegations ->
0 entries`, while the identical deploy on a chain with no boundary in reach survived — 1 entry, the
operator's aggregate reading 140 against a control of 100. The arm's rig now steps around it rather than
measuring it: with `--no-autopropose` **and** `--no-propose-on-deploy`, every block in that run is one the
driver asked for and no two are concurrent.

## 6. The falsifiers, and the mutation each was run against

Each was run red with the fix removed (cost accounting back in the sidecar and the merge's pass disabled),
green with it in place:

| test | red without the fix, because |
|---|---|
| `casper/tests/block_index.rs::a_merge_reproduces_a_branchs_post_state_including_its_native_writes` | the merge applies only the tuple-space `StateChange`s, so the reconstructed state is the block's post-state *minus* the native leaves |
| `casper/src/merging.rs::boundary_merge_tests::a_cost_accounted_block_keeps_its_own_native_writes_through_the_merge` | the concurrent sibling overlaps on `pos:vault`, one block is rejected whole, and the unique write goes with it |
| `casper/src/merging.rs::boundary_merge_tests::equal_concurrent_vault_writes_do_not_destroy_rev` | the two charges conflict instead of composing |
| `casper/tests/deploy_native_write.rs::a_merged_block_reproduces_its_own_post_state` | the merged hash differs from the block's own post-state — the strongest statement available, and it fails if the sidecar and the merge disagree by one leaf in either direction |
| `casper/tests/block_index.rs::a_slashed_validator_is_absent_from_the_bonds_at_a_merged_root` | the sibling pair conflicts, so the fixture can no longer run as the bare pair it now is |

That last one is worth naming: the fixture had been *shaped around* this defect. Its own comment
recorded that a bare pair of siblings both write the staking vault, one chain is rejected, and the
slash dies with it, so the test had to tell the merge that the slashing branch had seen the other. The
fix removed the need for that, and the fixture now runs with an empty `ancestry` map — which makes it a
falsifier for the fix rather than a workaround for the defect.
