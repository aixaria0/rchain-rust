# C219 — a minted channel keeps one continuation, so the vault handle's `balance` arm has never existed

Found 2026-10-05 while writing W2.5's reserve falsifier (`casper/tests/ertp.rs`): a REV round-trip
needed to *read* the balance of a minted vault handle over the reserve, and the read produced
nothing. The transfer arm of the same handle works, which is why the defect has survived: the
existing test (`casper/tests/determinism.rs::a_minted_vault_handle_spends_in_the_deploy_that_minted_it`)
asserts the handle's **effect** (the funds moved) and never its **reply**.

## The mechanism

`install_vault_handle` (`rholang/src/system_processes.rs`) installs two arms on **one** channel:

```rust
for (arity, handler) in [
    (2, vault_balance_handler(cc.clone(), native.clone(), address.clone())),
    (5, vault_transfer_handler(cc.clone(), native.clone(), address.clone())),
] {
    cc.install_native(name_bytes.clone(), arity, handler).await?;
}
```

`install_native` → `space.install` → `InMemHotStore::install_continuation`
(`rspace/src/hot_store.rs:319-323`):

```rust
state.installed_continuations.insert(channels.to_vec(), wc);
```

`installed_continuations` is keyed by the **channel**, holding one `WaitingContinuation`, so the
second install **replaces** the first. (A *deploy's* receives are unaffected: they accumulate in
`state.continuations`, a `Vec` per channel — `remove_continuation` indexes it as a list.) The
comment on `install_native` even states the opposite of what the store does: "RSpace matches by
arity, and the oracle's vault is exactly that shape: one `contract` per method".

## The measurement

A probe deploy, in a **block** over a genesis that funds the deployer with 100:

```rholang
new rv(`rho:rchain:revVault`), deployerId(`rho:rchain:deployerId`), hch, bch, tch in {
  rv!("findOrCreate", *deployerId, *hch) |
  for (@(_, *handle) <- hch) {
    handle!("balance", *bch) |
    for (@b <- bch) { @"handle-balance"!(b) } |
    handle!("transfer", "1111XuaDWqJtFmeR132nX6xY6rfMQrgNDjkmizUwDMRZzkvKmatY1", 10, *deployerId, *tch) |
    for (@t <- tch) { @"handle-transfer"!(t) }
  }
}
```

With the installs in their **committed order** (arity 2, then arity 5):

```
PROBE verdict: [[]]                    # the deploy succeeded, with no error at all
PROBE handle-balance: []               # the balance arm: silence
PROBE handle-transfer: [(true, Nil)]   # the transfer arm: replies
PROBE payee balance: Ok(Some(10))      # …and its effect lands
```

A **nonsense** method at the dead arity is silent too — no error, no reply:

```
                                       handle!("nonsense", *bch)
PROBE verdict: [[]]
PROBE handle-balance: []
```

With the two installs **swapped** (arity 5, then arity 2) the defect inverts exactly:

```
PROBE handle-balance: [GInt(100)]      # now the balance arm answers
PROBE handle-transfer: []              # and the transfer arm is gone
PROBE payee balance: Ok(None)          # …with no effect either
```

So it is not "arity 2 does not work": it is "the last install on a channel wins".

## Why it matters

* A client that calls `findOrCreate` and then reads the handle's balance gets the silent no-op this
  repository keeps meeting — an unmatched `for`, indistinguishable from a client bug.
* `vault_transfer_handler`'s own doc says its `(false, "Invalid AuthKey")` reply is "the `Either`
  shape both wallet vectors destructure" — that reply is unreachable when the balance handler is the
  one that survives, and vice versa. Whichever arm is second is the only one that exists.
* The `revVault` **Definition** path is not affected (one continuation at `arity: 1, remainder: true`
  with the method dispatched inside), and neither are ERTP's new REV ops: they are four methods of
  one such Definition, which is why `rho:rchain:ertp:ledger` answers all of them.

## The fix, when it is taken

The shape the rest of the system contracts use: **install the handle as one continuation at
`arity: 1, remainder: true` and dispatch on the method string inside it**, exactly as `rev_vault` and
`ertp` do. A 5-par `transfer` send matches that pattern (it is what matches the handle today, and the
determinism test pins its effect), and the 2-par `balance` send starts matching too — so the change
is backwards compatible on the wire and fixes the dead arm. It does move the continuation's
`body_ref`, which is derived from `(name, arity)` (`ContractCall::native_body_ref`), so it is a
**consensus-visible** change for newly minted handles and belongs in its own unit with a genesis
note rather than in a feature already under review.
