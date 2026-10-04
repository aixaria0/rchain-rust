# n220-leave — A2.5, a validator leaves: **established on the rig**

Rig: `n220-leave-run.sh`. **Tree `a077a87e4`**, image `363c0588…`, artefacts
`n220-leave-blocks/a077a87e4-20261004T141409Z/`. Three validators at 100/100/50,
`--epoch-length 10 --quarantine-length 20 --no-autopropose --propose-on-deploy`. `--quarantine-length` is
a node flag `tools/devnet.sh` does not expose, so it goes through `DEVNET_EXTRA_FLAGS`; the default is
50,000 blocks, far beyond any bounded run.

## The reading

| | |
|---|---|
| **L1 — the withdrawal stages** | ✅ **`deadline=30 blocks_remaining=20`** |
| **L2 — the boundary deactivates it** | ✅ active set 3 → 2 |
| **L3 — the entry clears** | ✅ no pending withdrawal remains |
| **L4 — the payout lands** | ✅ **vault `99998098` → `99998964`, +866, blocks 13 → 34** |

**Why the delta is the payout and nothing else.** Between the two reads validator 2 makes no deploy of its
own — every deploy in the window is signed by the **genesis** key, whose phlo is charged to its own vault —
so the only thing that can credit the account being read is `close_block`'s step 3: `payable = claim.bond +
committed_reward` paid into `vault_address(validator)`, with the escrow entry removed
(`rholang/src/native_state.rs:close_block`). **+866 = the 50-unit bond + 816 of committed reward.**

Both reads are recorded with **the block their datum was produced in**, and both probes record **their own
deploy's status** — `ok 13` and `ok 34`. So the comparison is between two readings the rig can *place in
the chain*, not between two numbers it happens to hold. The baseline is block 13, the post-read is block
34, and the deadline is 30: the payout block lies strictly between them.

## CH-U6-09 is settled, and by the API's own numbers

The challenge recorded that the worksheet's H-U6-05 **inverts** the quarantine arithmetic — *"the refund
then waits `quarantine_length` more blocks past its deadline"*. The read path answers with its own fields:
**`deadline=30`, `blocks_remaining=20`**, at a staging height of ~10 with `quarantine_length = 20` and
`epoch_length = 10`. That is `quarantine_length + divisor·(1 + block_number/divisor)` = `20 + 10·(1+1)` —
the recorded form, with the quarantine **inside** the deadline, and **not** the worksheet's. The
challenge asked for the arithmetic to be *read*, and it has been. **It resolves against the worksheet.**

## How it got here — three attempts, seven instrument faults, every one of them mine

This rig took four runs. **No failure was ever the chain's.** That is the pattern the whole acceptance
pass keeps re-learning, and it is worth listing in full, because each fault is a way a probe can lie:

**First pass — `a34b79d45`, artefacts `n220-leave-blocks/a34b79d45-20261004T112055Z/`.**
1. **The withdraw deploy could not pay its phlo** — `preCharge: insufficient funds (0 < 1000000)`:
   `tools/devnet.sh`'s genesis funds **only the deployer**, so validators 1 and 2 have empty vaults. Fixed
   the way the join rig fixes the same thing: fund first (`examples/leave-fund.rho`).
2. **`L1` passed on emptiness.** The detector used `[0-9]*`, which matches *zero* digits, so `deadline=`
   with nothing after it "matched" and the rig printed `L1 PASS` with two empty fields while no withdrawal
   existed. **A witness that passes on emptiness is not a witness.**
3. **The read path's field is `pendingWithdrawals`** — plural, camelCase, an **array** — and the rig read
   `pending_withdrawal`. It reported a *failure* on a withdrawal that had in fact taken effect.

**Second attempt — `d1b0a5da1-20261004T134046Z`.** `L1`/`L2`/`L3` passed and **`L4` read `0` before and
after** on an account funded 100,000,000. Two faults, both proved rather than suspected:
4. **The channel was reused, so the second read was the first read's datum.** `listen-data-at-name`
   returns **what is already at the name** and waits only if there is nothing — it is not a
   subscribe-to-changes read, which is what the rig (and its own comment) assumed. The stale datum is
   identifiable in the probe's capture: `block_number: 5`, three blocks *before* the fund (7) and nine
   before the withdraw (14). A distinct channel per read is not a stylistic choice.
5. **The baseline was taken before the fund.** `b_pre` was read at a time when validator 2 was legitimately
   unfunded, so `after > before` would have moved by the whole 100,000,000 and could not have told the
   **fund** from the **payout**. The baseline now runs *after* the withdraw deploy, and doubles as the
   probe's control: it must read close to the funded figure, and `99998098` does.

**Third attempt — `d1b0a5da1-20261004T135701Z`.** The post-read came back `unreadable`.
6. **The probe discarded its own deploy's status**, so a deploy that never landed and a chain that never
   produced a block were indistinguishable — and the run could not say which. `balance()` now deploys
   **first**, waits for `deploy_status` to read `ok`, and only then reads (which returns immediately,
   because the data is already at the name). That also removes the listener/deploy race entirely.
7. **The drive loop stopped at "the pending entry cleared" instead of "the head is past the deadline".**
   `pending_withdrawers` clears at the epoch boundary — `close_block` moves the request into
   `withdrawers`, ~10 blocks after the staging and ~20 before the payout — so a loop keyed on it stops
   **two epochs early** and reads the vault before anything has been credited. The loop now drives until
   `height > deadline`, and the witness records it: `drove to height 34; deadline=30, past-deadline=1`.

## Limits — what this is not

One tree, one host, one complete mesh, one silent window; the withdrawal is a **validator's**, not a
delegator's. The epoch boundary is 10 blocks and the quarantine 20, both shrunk from production values, so
nothing here speaks to the timings a real net would see. And it is a **rig** result: it says the payout
path pays on a net where nothing is late. It does not say a validator can leave safely on a **live** net —
that is the live-arm question, and the page keeps the distinction it applied to its own A2.2.

**Files.** Rig `spec/audit/evidence/n220-leave-run.sh`; probes `examples/leave-fund.rho`,
`examples/leave-balance.rho`, `examples/leave-balance-after.rho`. The decisive run's node logs are
committed with it; the two superseded attempts keep their witnesses, series and probe captures, without
~100 KB of node logs each.
