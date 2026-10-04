# n220-leave — A2.5, a validator leaves: **partially established**

Rig: `n220-leave-run.sh`. Artefacts: `n220-leave-blocks/a34b79d45-20261004T112055Z/`.
Tree `a34b79d45`, image `363c0588…`. Rig: three validators at 100/100/50, `--epoch-length 10`,
`--no-autopropose --propose-on-deploy`, and **`--quarantine-length 20`** — a node flag `tools/devnet.sh`
does not expose, so it goes through `DEVNET_EXTRA_FLAGS`; the default is 50000 blocks, far beyond any
bounded run.

## What it establishes

| | observation |
|---|---|
| the withdraw **is processed** | the deploy returns `ok`, block 9 |
| the validator **leaves the active set** | `activeValidators` 3 → **2**, and stays 2 for the rest of the run |
| the chain stays healthy afterwards | height 164 → 164 over a 60 s quiet read, finality tracking |

So the *leave itself* works: a staker's `withdraw` is accepted, a boundary deactivates it, and the net
does not wedge. That is more than nothing — it is the first live look at the path.

## What it does **not** establish, and why this is a ⬜ rather than a ✅

**The payout is not observed, and neither is the quarantine arithmetic.** A2.5's witness is *the payout
transfers*, and the rig's observable for it was `/api/v1/pos`'s `pendingWithdrawals` entry — which was
**absent in all 49 samples**, before and after. Two readings, and this run cannot choose between them:

- the withdrawal took effect and its pending entry was **never exposed** by the read path — which would be
  a read-path gap worth its own finding; or
- the entry appeared and cleared **between two samples**, which a 12-second cadence makes unlikely but not
  impossible.

What makes the first reading live rather than dismissed: the deactivation is *immediate* at the boundary
while the quarantine is 20 blocks, so an entry should have been visible for roughly two samples had the
read path carried it. It did not.

**CH-U6-09 is therefore not settled.** That challenge records that the worksheet's H-U6-05 inverts the
quarantine arithmetic ("the refund waits `quarantine_length` more blocks past its deadline"), and the
settling observable was the API's own `deadline` / `blocksRemaining`. Neither field was ever populated for
this withdrawal, so the arithmetic is **unread**, not confirmed and not refuted. It stays open.

## Three instrument defects, all mine, all recorded

This rig took three runs, and **each failure was the instrument rather than the chain** — the pattern this
whole pass keeps re-learning:

1. **The withdraw deploy could not pay its phlo.** `preCharge: insufficient funds (0 < 1000000)` —
   `tools/devnet.sh`'s genesis funds **only the deployer**, so validators 1 and 2 have empty vaults. Fixed
   the way the join rig fixes the same problem: fund first (`examples/leave-fund.rho`).
2. **`L1` passed on emptiness.** The detector used `[0-9]*`, which matches *zero* digits, so `deadline=`
   with nothing after it "matched" and the rig printed `L1 PASS` with two empty fields while no withdrawal
   existed. **A witness that passes on emptiness is not a witness.**
3. **The read path's field is `pendingWithdrawals`** — plural, camelCase, an array — and the rig read
   `pending_withdrawal`. A reader that cannot see the thing it tests is worse than no test, and this one
   reported a *failure* on a withdrawal that had in fact taken effect.

## Limits

One tree, one host, one attempt; the withdrawal is a **validator's**, not a delegator's, and the read
path's `pendingWithdrawals` behaviour for that case is exactly what is in question. The epoch boundary is
10 blocks and the quarantine 20, both shrunk from production values, so nothing here speaks to the
timings a real net would see.
