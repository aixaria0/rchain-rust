# n220-silent-join — a validator bonds and never speaks: **passes**

Rig: `n220-silent-join-run.sh`. Artefacts: `n220-silent-blocks/` (tree `3a3536c3f`, in its `manifest.txt`).
Tree `3a3536c3f`, image `363c0588…`. Three live validators at 100/100/50, `--epoch-length 10`,
`--no-autopropose --propose-on-deploy`.

**The rig is the join rig minus the node.** `join-admit.rho` funds key 3's address and `join-bond.rho` is
*signed by key 3 but sent to the bootstrap* — deploys are not gossiped — so **neither needs validator-3 to
be running**. `n220-join-run.sh` starts it with `devnet.sh reset 3` only because it wants to watch the
newcomer produce; here that step is omitted on purpose, and the run confirms no fourth container ever
existed (`3 up`, and `3` after the bond). So a validator sits in the bond pool that speaks to nobody —
the shape #213 is named for, and the one `A2.4`'s caveat said was unrun.

## The reading

| | |
|---|---|
| the bond lands | **S1 PASS** — a block's `bonds` name the silent validator from block 10 |
| finality **at** the bond | 7 |
| **the three live validators finalise past it** | **S2 PASS — 7 → 10** |
| quiet afterwards | height 27 → 27 over a 60 s read |
| containers | 3, before and after — the fourth never existed |

**It does not wedge the chain.** The pool is 100 + 100 + 50 + 50 = 300 after the bond; the three live
validators hold 250 of it (83 %), a supermajority of the whole bonded map, and the silent one is retired
from the *live partition* by `LIVENESS_WINDOW` — which is what Law 52b's clauses are about (`Void` when
the partition cannot shrink, and it can).

**This is the answer `A2.4`'s caveat asked for**, with the same proviso the rest of the page now carries:
it is a **rig** result, one host, complete mesh, no delivery delay. It says a silent bonded validator is
not *by itself* a wedge. It does not say a silent validator on a live net cannot contribute to one — that
is #223's question and it is measured there, not here.

## What it does not cover

- **One silent validator of four.** Not two of four (which drops the live set to 150/300 = 50 %) and not
  a silent validator holding a *large* share.
- **A silent validator that never spoke at all**, rather than one bonded after genesis — though the two
  differ only in when the partition first has to retire it.
- Delivery delay and reordering: one host, complete mesh, as the pre-registration notes elsewhere.

## The A2.5 misreading this pass also corrected

Not this rig, but found while reading the same read path, and it corrects a conclusion I published on
#214 and #223: **`/api/v1/pos`'s `pendingWithdrawals` is empty for most of a withdrawal, by design.**
`pos:pending_withdrawers` holds requests only **until the next epoch boundary**, where `close_block` moves
them into `pos:withdrawers` and takes the validator out of the pool
(`rholang/src/native_state.rs:95-98`). With `--epoch-length 10` and a withdraw staged at height 9, that
window is **one block** — and the leave rig's 15-second sleep between the deploy and its first sample
missed it. So the absence I reported is **expected behaviour, not a read-path gap**, and the payout is
observable by the **vault balance** (as `pos-balance.rho` does for a delegator), which the leave rig did
not read. A2.5 remains ⬜ — but for a reason that is now understood, and with a known next step rather
than a suspected defect.
