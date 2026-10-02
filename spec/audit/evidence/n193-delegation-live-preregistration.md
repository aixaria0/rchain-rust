# #193's live arm — pre-registration

**Written before the run, and committed before it too**, because the value of a measurement on this rig
is that its success criteria were fixed while they could still be wrong. The driver is
`n193-delegation-live-run.sh`; the results are `n193-delegation-live-results.md`.

## What is measured

Delegated stake (law 57, #193) end to end on a two-validator devnet, in the order the issue's own close
condition names it. Validator 1 holds REV at genesis (the devnet's deployer is validator 1 by default,
and genesis funds its address), and it delegates **40** of it to **validator 2's key** — a key it does
not control, which is the whole point of the primitive.

1. **`delegate` accepts on a live network** — the deploy's own `(Bool, Either)` reply is `true`.
2. **The aggregate reaches the chain.** Validator 2's `pos:bonds` entry goes **100 → 140**, visible in a
   block's own `bonds` map — which is the number `compute_bonds` recomputes from `pos:active` and
   `Validate::bonds_cache` checks the block against. This is the "no Casper change was needed" claim
   observed rather than argued.
3. **A boundary splits the reward**, the operator keeping the remainder.
4. **`undelegate` → boundary → quarantine payout**, and the payout lands in the **delegator's own**
   vault, carrying the delegator's accrued reward — never in the operator's.
5. **The fork point** (`RUN_DIVERGENCE=1`, needs an unupgraded `rnode:old`): the same deploy with
   validator 2 on the **old binary**, whose `rho:rchain:pos` has no `delegate` arm. It cannot reproduce
   the block's post-state, so it refuses the block while the other nodes accept it.

## Success criteria, fixed in advance

- **C1** the `delegate` reply is `true`, read back from the public name `pos-delegate`.
- **C2** a block's `bonds` map carries stake `140` for validator 2. **If it does not, the delegation
  never reached consensus**, which would mean the pool write or `select_active` is wrong — a failure of
  the primitive, not of the measurement.
- **C3** after the undelegation and one boundary past its quarantine, the operator's entry is back to
  **100**.
- **C4** the delegator's vault is **credited**, and the operator's is not, by the delegation payout.
- **C5** (divergence only) validator 2's height falls behind the other two and its log carries a refusal.

## Stop condition

The run is abandoned — and reported as abandoned rather than as a negative result — if the network does
not reach two live validators, or if a `show-blocks` dump comes back under 100 lines. That is the same
refusal the A1/A2 drivers adopted after their first versions reported an *absence their own command had
produced*: a check that reads a failed command's empty output as "the property does not hold" is worse
than no check. One attempt is run; a second is run only if the first fails for a reason this file did
not anticipate, and the results file says so if it happened.

## What this arm does not cover, stated here so the results file cannot imply it

- **Not the wire between an old and a new node in general** — only the one deploy that carries the new
  method. C5 is a divergence, not a general compatibility claim.
- **Not a large network.** Two validators, so the draw never selects and the split's delegator count is
  one. The `O(delegators)` costs residual O5 names are not measured here.
- **Not an economic measurement.** The income table and the concentration finding are simulated, in
  `spec/RUST-VS-SCALA.md` §3 item 12 and the book; nothing here prices anything.

## Why the old image is not built by the driver

`rnode:old` is a second full `cargo build --release` inside Docker — roughly ten minutes and two
gigabytes on this host, which has a documented history of failing under concurrent heavy work. The
driver therefore **refuses** (`exit 2`) rather than silently skipping phase 5 when the image is absent:
a phase that quietly does not run is the failure mode the rest of this file exists to avoid. The build
command is in the driver's header.
