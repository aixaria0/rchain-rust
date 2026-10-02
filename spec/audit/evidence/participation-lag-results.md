# The participation lag on a live devnet (C203, #150)

The measurement the register owed: what a **live** validator's participation lag actually is, so that
`absence-slack` (the knee) and `participation-grace` (the flat part below it) can be chosen against a
number rather than a guess. The instrument is `close_block`'s own log line — the participation map is an
argument to it since the fringe read landed, so a boundary can report the drawn set's lags in order.

Run under `spec/audit/evidence/participation-lag/`: `bootstrap-boundaries.txt` is every boundary line the
bootstrap printed, and `status.json` is the node's own report at capture.

## What it was run on

`tools/devnet.sh up --epoch-length 1`, twice — first with the default single validator, then with
`--validators 3`. `--epoch-length 1` makes every block a boundary, so the instrument reports on every
block; the *lag* it reports is a property of the chain, not of the epoch length, so the shorter epoch is
what buys samples rather than what changes the measurement.

**Two caveats about the rig, and the first is the one that limits what this can say.**

* **`drawn=1` on both runs**, including the three-validator one. So the drawn set is a single validator
  and the **round-robin term is absent** from these readings: what varies across a validator's own turns
  — the thing a slack has to cover on a chain with a large active set — is not in the data below. The
  reading is the *floor*, not the distribution.
* The node reports its version as `254c837dc`, one commit behind the tree the image was built from: the
  Docker build copies the working tree, and the instrument was in it before it was committed. The binary
  under test is the tree with the instrument; only the stamped string lags.

## The reading

**350 boundary lines, and every one of them is the same:**

```
[pos] participation lag at boundary N: drawn=1 never_spoke=0 min=4 median=4 p90=4 max=4
```

A constant lag of **4 heights** behind the last finalised fringe, from boundary 69 to the end of the run
at height 244, on a chain carrying live validators throughout.

## What that means, and it is not what one would guess

**The lag of a live validator is the finality lag, not zero.** The participation is read at the *last
finalised* fringe, so a validator that has just spoken — that spoke in this very round — still reads as
far behind as it takes finality to advance. On this rig that is 4; the tree's own measurements put the
finality gap at a constant 4 on a live chain (`spec/audit/evidence/n148-results.md`'s all-live arm), so
the two agree.

**That is the sharp consequence for the two parameters, and it inverts the obvious choice.** A network
picking a knee from intuition — "a validator that missed a round or two should be cut" — would pick
something like 5 or 10, and on a chain with a finality lag of 4 it would be **taxing validators that
missed nothing at all**. The value has to be *the finality lag plus the silence a network wants to
price*, and the first term is not a property anybody can read off the configuration.

## What this does not establish, stated rather than implied

* **Not a value to ship.** The lag depends on the block rate, the active-set size and how fast finality
  advances, all of which are the network's — so a number measured here is a fact about this rig. What
  generalises is the method (the instrument) and the *shape* of the answer (a floor at the finality lag).
* **Not the multi-validator distribution.** `drawn=1` throughout, so the round-robin widening is absent.
  A network with a large active set has to run this itself on its own genesis parameters — which is what
  the instrument is for, and why it logs whether or not the rule is armed.
* **Not a defect.** Nothing here says the read is wrong; a lag measured against the last finalised fringe
  is what the design is. It says the *value* is a two-term sum, and the larger term is invisible from
  the outside.
