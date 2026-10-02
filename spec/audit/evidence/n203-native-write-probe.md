# #203 — what a deploy can see of native state, and an over-claim corrected

Run 2026-10-02 on tree `17966d93` (the delegation branch), two-validator and **one-validator** devnets
built from that tree, driven by the probes in `examples/` (`pos-trust.rho`, `pos-trusted.rho`,
`pos-withdraw.rho`, `pos-bonds.rho`).

**This file exists because the first write-up of this finding claimed more than the measurements
support, and the correction is the most useful thing in it.** Read §3 before §2.

## 1. What was run, and what it showed

`pos!("trust", *deployerId, "<65-byte key>".hexToBytes(), *ret)` from an ordinary deploy, then a
**separate, later** deploy of `pos!("getTrusted", *ret)` reading the set back:

- the `trust` op replies `(true, Nil)` — read back from the public name `@"pos-trust"`, so the op
  accepted rather than merely reducing successfully (a refusal returns a tuple too, which is why the
  reply and not the log is the evidence);
- the later `getTrusted` deploy's reply **does not contain the trusted key**;
- and this reproduces with `--no-autopropose` (no dummy deploys) on **one** validator (no second
  branch), with the node logging **`0 merges`** over the whole run.

## 2. The claim that was drawn from that, and why it was too strong

The first version of AUDIT C205 said *the write is absent from the node's native state*. That is not
what was measured, and a control contradicts it.

`withdraw` writes `pos:pending_withdrawers`, and `GET /api/v1/pos` exposes it. On the same network, the
same minute:

```
$ curl -s http://localhost:40403/api/v1/pos
"pendingWithdrawals":[{"validator":"04f700a417754b77…","stagedAtBlock":60000,"blocksRemaining":109996}]
```

The `withdraw` deploy's reply was `(true, Nil)`, same as `trust`'s — and its write **is visible in the
node's live native store**, read without any deploy and without any block. So the node does hold a
native write made by a user deploy. The first write-up generalised from `trust`'s absence to a claim
about the state, and `withdraw` refutes that generalisation.

**The honest statement of what is measured is narrower and stranger:**

> A native write made by one user deploy is **visible to a non-deploy read of the same node's native
> state**, and **invisible to a later user deploy's read of it**.

Both halves were measured on one network within a minute of each other, and they cannot both be true of
a single consistent view. Something gives the block path and the HTTP route different native state.

## 3. What is eliminated, and how

- **Not the base block path.** `casper/tests/deploy_native_write.rs` plays the same op through
  `RuntimeManager::compute_state` — the proposer's own entry point — and reads the write back out of the
  post-state it commits to. **It passes.** Written as a falsifier and green, it says the loss is above
  this layer; it is kept as a regression guard.
- **Not the multi-parent merge.** Reproduces on one validator.
- **Not any merge at all.** The run logging `0 merges` reproduces it.
- **Not the autopropose deploy / the dev-mode keep-alive.** Reproduces with `--no-autopropose`.
- **Not the store backend.** `checkpoint_with_native` has one implementation, shared by the in-memory
  and durable repositories.
- **Not the probes.** The first version of `pos-trust.rho` carried a 67-byte key where `trust` requires
  65, so the deploy *failed* and read exactly like a reproduced bug. Found by reading the
  `[deploy] … FAILED` line instead of the count of successful-looking lines.

## 4. What this means for #193, restated

C204's live arm could not distinguish "the delegation did not take effect" from "the delegation took
effect and the read could not see it" — because `getBonds` is a **deploy** read, and deploy reads are
the half that is stale here. So C204 stays open on a defect this file narrows rather than on one it
identifies, and the delegation itself is neither cleared nor implicated.

## 5. Where the next unit starts

The two reads disagree, so the question is **which native state a deploy's read is given**. The
candidates, in the order this investigation would take them:

1. `RSpace::reset` re-points `native_store` at a `start_hash` reader *and* clears the overlay — so a
   block whose `start_hash` reader predates the previous block's native changes would read exactly this.
   Compare the reader a block path is handed with the one `/api/v1/pos` reads.
2. `save_native_changes` / `load_native_changes` keyed by `(post_state_hash, sender, seq_num)`: whether
   the entry a block reads was written by the play path, the replay path, or `regenerate_sidecars`, and
   whether those three agree.

Both are checkable in-process — the falsifier in `casper/tests/deploy_native_write.rs` is the harness
for it, extended one step further along the node's pipeline than `compute_state`.
