# #203 (C205): the defect does not exist — a void measurement, and the instrument that closed it

Run 2026-10-02 on tree `1f7b1697` (branch `c205-native-write`), one- and two-validator devnets built
from that tree, `--no-autopropose`, blocks placed by `--propose-on-deploy` and **confirmed from the
node's own log** before any reading was taken.

## 1. The result

**C205 is an artifact and closes.** Two measurements, each with a positive control in the same reply:

| what | measurement | control | verdict |
|---|---|---|---|
| a deploy in block **1** trusts a key; a deploy in block **2** asks whether it is trusted | `trusted.contains(newcomer)` → **`true`** | `trusted.contains(genesis validator)` → **`true`** | the write is **visible** to a later block's deploy |
| a deploy in block **2** delegates 40 to validator 2's key; a deploy in **block 3** asks what that key carries | `bonds.get(operator)` → **`140`** | `bonds.get(validator 1)` → **`100`** | the aggregate is **visible** to a later block's deploy |

So a user deploy's native write **is** seen by a later block's deploy, on a live network, for both the
`trust` leaf and the delegation ledger's effect on the pool. Nothing is lost and nothing is stale.

## 2. Why the earlier reading said otherwise — the whole finding

`tools/devnet.sh query` reads through `listen-data-at-name`, which prints `{result:?}` — a **derived
`Debug`**. A 65-byte key therefore renders as `GByteArray([2, 2, 2, …])`, **decimal bytes**
(`models/src/rholang.rs:40`), and **no hex grep can match it**. Every "the newcomer is absent"
conclusion in the previous write-up came from grepping that output for `0202…`. The hex strings that
*did* match (`04f700a4…`, `9f52f05d…`) are from the surrounding `LightBlockInfo`, which is a different
structure on the same line rendering in a different format.

**Nothing about the set was ever inspected.** The reading was void, and it was published as a negative
result, twice — as AUDIT C205 and as a correction to it that narrowed the wrong thing.

Two further confounds were live at the same time, and both are now eliminated by construction rather
than by argument:

- **Deploy ordering.** `--no-autopropose` does **not** disable `--propose-on-deploy` (`tools/devnet.sh`
  — the flag flips only `AUTOPROPOSE`), the deploy CLI returns on `ProposerResult::Started` *before the
  block is built*, and the pool is a `BTreeMap` keyed by deploy **signature** — so "later" was
  wall-clock order, not execution order. The runs above take the block numbers from the node's own log
  (`proposed and added block #N`) and require the two deploys to be in different blocks.
- **The pre-state lead is retracted.** A previous dump showed 49 blocks with only 3 distinct
  `preStateHash` values, which looked like blocks building on a stale state. On a controlled chain the
  same dump is **linear**: block 1's `postStateHash` is block 2's `preStateHash` is block 3's, and
  empty blocks legitimately share a post-state. The two-validator dump was read as if it were a chain;
  it is a DAG in which concurrent blocks share a parent.

## 3. The instrument, which is the durable part

`examples/pos-trusted.rho` and the probe pattern it fixes: **compute the answer inside rholang and
return a boolean, with a control in the same reply.**

- `trusted.contains(k)` is `ESet`'s own membership test (`rholang/src/reduce.rs:1329-1356`), so the
  comparison happens on the values and no rendering is involved.
- **The second element is a positive control.** It asks about a key that is trusted *by construction*.
  A reply of `(false, false)` means the instrument is broken, not that the subject is absent — which is
  the fault that produced this whole episode. The first version of this probe had no control, and its
  silence read as a finding.

`examples/pos-bonds-check-live.rho` applies the same shape to a number: the operator's entry and an
untouched validator's, so a reply that is wrong in the same direction as a broken read is visible.

## 4. What the four-lens root-cause analysis produced, given there was nothing to root-cause

Run before this phase, at the requester's direction: four investigators on distinct lenses (reader
reset, write path, runtime identity, adversarial). All four converged on a **stale pre-state hand-off**
as the only mechanism that could produce the stated symptoms — and the identity lens mapped every
runtime and store in a running node precisely enough that its table is worth keeping for the next
investigation of this area. **The adversarial lens is the one that ended it**: it identified the
rendering confound (decimal `Debug` on that surface) and the deploy-ordering confound, and it stated
the observation that would settle the question. That observation was one run, above.

**The lesson worth carrying: the analysis was correct to be suspicious of the measurement, and the
three mechanism-building lenses were all building on a reading none of them could have checked.** The
cheapest test was also the decisive one, and it should have come before any mechanism work.

## 5. What is actually wrong, from the same runs

One real defect, found while building the control and unrelated to any of the above:

**`GET /api/v1/pos` misreports a withdrawal's timing.** `withdraw` stores the **deadline**
(`quarantine_length + divisor·(1 + block_number/divisor)`, `rholang/src/native_state.rs:1385-1392`),
while `pos_read.rs:31-33` labels it `stagedAtBlock` and `:137-140` computes
`blocks_remaining = staged_at_block + quarantine_length − latest` — adding the quarantine a second
time. Measured live: `{"stagedAtBlock":60000,"blocksRemaining":109996}` at chain height 4, where the
truth is a deadline of 60000 and about 59996 blocks remaining. **The operator's number is wrong by
50,000 blocks**, on the route AUDIT C148 exists to provide. Filed separately.

## 6. What this means for #193

**C204's blocker is gone and its live arm can proceed.** The delegation was never implicated: the
`getBonds` reading of 100 that stalled it was taken through the same unreliable route, and under the
block-verified instrument the same operation reads **140**. So the delegation primitive works end to
end on a live network as far as this probe goes — bond→delegate→a later block's read — and C204's
remaining criterion is the full path (boundary split, undelegate, quarantine payout) plus the
unupgraded-node fork point.
