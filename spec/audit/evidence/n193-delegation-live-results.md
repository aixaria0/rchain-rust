# #193's live arm — results: **the arm failed, and the reason is a pre-existing defect**

> **⚠ SUPERSEDED 2026-10-02. The defect this file reports does not exist, and C205 is closed as an
> artifact.** Every *absent* reading below came from grepping `listen-data-at-name`'s output for hex, on
> a surface that renders a 65-byte key as `GByteArray([2, 2, …])` — **decimal bytes**. Under an
> instrument that computes the answer in rholang with a positive control, the same operation reads back
> correctly (`spec/audit/evidence/n203-native-write-probe.md`). The file is kept **unamended below its
> own corrections**, because a record that edits away the reading it got wrong is not a record; read the
> probe file as the authority and this one as what was believed at the time.

Run 2026-10-02 on tree `1a5c9539c` (the delegation change, `delegation/2-rust` + the arm's own rig
change), against the pre-registration `n193-delegation-live-preregistration.md`. Image built from the
tree under test (`rnode:local`, the `GIT_HEAD_COMMIT` build-arg carrying `1a5c9539c`).

**Two runs, and only the first is the driver's** — `n193-delegation-live.log.txt` is its log. Run B is
the third control below (the default epoch length); it was **typed by hand** into the same rig rather
than re-driven, because the question it answers — does a boundary destroy the write, or did it never
land — needed one deploy and one read. Its evidence is in this file and in the transactions the run
made, not in the driver's log, and saying so is the difference between a record and an impression.

**No success criterion was met, and the reason is not the delegation primitive.** The delegate op runs,
reports success, and its post-state write is **not visible in the node's own native state afterwards**.
The same is true of `withdraw`, which is pre-existing code — so what this arm found is a property of
**native-state writes made by an ordinary user deploy**, not of #193's addition.

## 1. What happened

Run A (`--validators 2 --epoch-length 2`), the bootstrap's log, in order:

```
[pos] called: delegate with 4 argument(s)
[pos] ok
[deploy] deployer=04f700a417754b77 ok cost=2697
… proposed and added block #1 (seq 1)
```

and the block itself carries the deploy — `deploy_count: 1`, `rejected_deploys: []`, and a post-state
hash that differs from its pre-state. So the deploy was **executed, committed and accepted**, and the
op returned `Ok`.

Then the chain disagrees about the stake. Across 49 blocks (98 bond entries) every entry reads
`"stake": 100`, and no block carries the 140 that C2 asks for. At height ~77 a direct pool read —
`pos!("getBonds", *ret)`, the probe in `examples/pos-bonds.rho` — answers **100 and 100**.

**C1 was met** (the deploy's own reply is `true`, read back from the public name). **C2, C3, C4 were
not**; C5 was not attempted, because it depends on the network agreeing in the first place.

## 2. The controls that make this a finding rather than a flaky run

Three, and each rules out a way the observation could have been the instrument's:

- **The same deploy's tuple-space effect persists.** `tools/devnet.sh query pos-delegate` reads back the
  `true` the deploy published on `@"pos-delegate"`. So the deploy's effects were not wholly discarded —
  **in one deploy, the rholang send survived and the native write did not**.
- **`withdraw` behaves the same, and it is not this change's code.** `pos-withdraw.rho` (checked in
  before #193) reports `[pos] called: withdraw` / `[pos] ok` / `[deploy] … ok cost=1902`, and then
  `GET /api/v1/pos` answers `"pendingWithdrawals": []`. **⚠ This control does not survive the follow-up
  probe and must not be relied on**: on a later run the *same* deploy against the *same* route answers
  `"pendingWithdrawals":[{"validator":"04f700a4…"}]`, so what this sentence recorded was not a property
  of `withdraw`. `n203-native-write-probe.md` §2 has the correction and is the authority; this bullet is
  left standing, marked, rather than quietly removed, because a record that hides the reading it got
  wrong is not a record.
- **It is not a boundary rewriting the ledger.** Run B brought the network up with the **default** epoch
  length, so no boundary ran at all, and deployed the same delegation: `getBonds` again answers
  **100 and 100**. The write is gone before any `close_block` could touch it.

## 3. What this is, as far as this arm can say

**Measured:** a native-state mutation performed inside an ordinary user deploy is absent from the node's
native state once the block carrying it is committed, while that same deploy's tuple-space mutation is
present, and the op reports success. It reproduces on two networks and on an op that predates the
change under test.

**Named, but not measured:** the mechanism. The repository already carries this symptom as a known one —
`casper/src/runtime_manager.rs` logs the deploy-level outcome separately *because* "a deploy whose state
changes are reverted … still logs `[pos] ok` from inside the call, **which is how a `trust` that
'reported success' left the trusted set unchanged (#74)**". The fix for #74 is the
`NativeChangesStore` sidecar (`casper/src/storage.rs`), whose own comment in `rspace.rs` says the drained
mutations are kept "**replacement, not accumulation**". A merge path that re-applies tuple space
correctly and native changes only from the last drain would produce exactly what is measured here. **That
is a hypothesis this arm did not test**, and it is written as one rather than as the cause.

## 4. What this means for #193

- **The primitive's close condition cannot be met on this tree**, and not because of anything in the
  primitive: a devnet run cannot exercise `delegate → boundary → undelegate → quarantine payout` while a
  deploy's native writes do not reach the state.
- **#193 stays open, and so does C204.** The spec unit (the mechanism, law 57 and its eleven theorems)
  and the Rust unit (the four leaves, the two ops, the split, the fan-out, the refusals, thirteen
  falsifiers that each went red under a mutation) are unaffected: they are correct at the layer the unit
  tests exercise, and those tests call `NativeSystemState` directly — which is exactly the layer this
  defect is *below*.
- **Every PoS op whose effect a deploy reads back is unreliable** until it is fixed — `getBonds`,
  `getActiveValidators`, `getTrusted`, and the `(Bool, Either)` reply of `bond`/`withdraw`/`trust`/
  `delegate`. **In the corrected form** (`n203-native-write-probe.md`): the writes are not shown to be
  lost, but the *reads* disagree with the node's own view, and the wallet's entire interface is a deploy
  reading the state back. That makes the finding more consequential than the primitive that exposed it,
  and it is filed as its own row rather than folded into #193's.

## 5. What the arm leaves behind

- The rig change it needed: `tools/devnet.sh` now takes a **per-node image**
  (`RNODE_IMAGE_<container>`), mirroring `DEVNET_EXTRA_FLAGS_<container>`, so an unupgraded node can run
  beside an upgraded one. That is a precondition for phase 5 whenever it is run.
- `examples/pos-delegate.rho` / `pos-undelegate.rho` (the call shapes, with the operator key
  substituted per run) and `examples/pos-bonds.rho` (the pool probe this diagnosis needed and nothing
  else provided).
- The driver's own bug, found and fixed in the same session: `say "… `(true, Nil)` …"` ran
  `(true, Nil)` as a command, so a message *about* the reply printed the shell's error instead. The
  backticks are escaped now. It did not affect any observed number.

---

# Re-run 2026-10-03, after AUDIT C207's fix — **green, end to end**

Run on tree `6def51794` + the boundary instrumentation (`fix/c207-sidecar`), image built from that
tree, two-validator devnet at `--epoch-length 2 --quarantine-length 1`. Log:
`n193-delegation-live.log.txt`. The driver exits 0.

| phase | reading | instrument |
|---|---|---|
| 1. `delegate` | `GBool(true)` | the op's own reply, off `@"pos-delegate"` |
| 2. the aggregate, read by a **later** block's deploy, with a control in the same reply | `GInt(140)` for the operator, `GInt(100)` for an untouched validator | a deploy, computed in rholang |
| 3. the wallet's HTTP route | `amount=40, accruedRewards=337, pendingUndelegation=null`; `[]` for a key that never delegated | the route the wallet's position screen calls |
| 3b. the same read in rholang | agrees | a deploy |
| 4. the undelegation's **staging** | `GBool(true)` and the ledger still carries the entry | read **in the block that staged it** |
| 5. the boundary's move | `[pos] boundary 42: undelegation escrowed — 40 out of the operator's pool entry, which now reads 100` | the boundary's own log line |
| 6. the payout | `[pos] boundary 28: undelegation paid — 2059 to the delegator's own vault (40 principal + 2019 accrued)` | same |

Phase 2 is the C207 falsifier end to end: before the fix this read `GInt(100)`/`GInt(100)` on this
same rig, because the deploy's write was rejected with its block.

## Why phases 5 and 6 are asserted from the node's own log

**A read of the chain is not the chain.** Every read in this arm that is not a deploy goes through a
surface that holds *a* state rather than *the* state, and on a forking rig it can land on one the
boundary did not write. Measured here: while the boundary's log said the operator's pool entry had
gone to 100, a deploy-based `getBonds` still read **140**, and the HTTP position route read `[]` for a
ledger a deploy-based `getDelegations` read as `1 entry`. A line the boundary printed *while writing
the state* cannot be a stale read, which is why step 2b and step 3b now report themselves — the same
reason the participation lag does. Both are ordinary `eprintln!`s on a consensus path and they are
what made this arm's last two phases legible.

## The instrument defects this arm forced, each one a real misreading of the chain

1. **An accumulating channel.** A probe's send accumulates and `listen-data-at-name` returns the
   oldest datum, so the second read of a name returned the *first* reading back — the payout phase
   compared a balance before and after eleven blocks and read the same number twice, byte for byte.
   `ask()` now publishes to a name of its own per call.
2. **A fee the assertion ignored.** The balance probe is itself a deploy by the account being read, so
   the deploys between two readings cost more than the principal being returned and "the balance must
   rise" is unsatisfiable by construction. The reading's own cost is now measured by two consecutive
   readings and subtracted.
3. **`--no-autopropose` alone is not a linear chain.** `--propose-on-deploy` fires on every gossiped
   deploy, so both validators propose and a boundary height races — and a *boundary* race discards one
   block whole, deploy included. Both flags are now off.
4. **`getDelegations` answers with a list**, so the in-block count is `.length()`; `.size()` fails the
   whole deploy and reverts the staging with it.
5. **A `sed` with no file.** Every one of these produced a reading that looked like the defect.

## What is still not covered

Not the wire: the two nodes are real containers, but every assertion is taken on the bootstrap's own
state or its own log. Not multiple delegators, and not a delegator whose operator is slashed mid-flight
— `rholang/src/native_state.rs::a_slash_fans_out_to_each_delegators_own_vault_at_every_tier` and
`an_undelegation_is_staged_then_paid_to_the_delegator` carry those in process. And not the fork point
(phase 6 of the driver, `RUN_DIVERGENCE=1`), which needs a second image built from the pre-change tree
and is unchanged from this file's own account of it.
